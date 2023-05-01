//! [Metrics] recorder that outputs metric updates as [tracing] events.
//!
//! This recorder is mostly useful for debugging and testing purposes. It allows
//! outputting structured logs for the metrics produced by the application
//! (with tracing spans for context and other good stuff). The produced tracing events
//! can also be captured using the [`tracing-capture`] crate and tested.
//! The `tracing-capture` crate provides dedicated support to "parse" metrics
//! from the tracing events; see its docs for details.
//!
//! [Metrics]: https://docs.rs/metrics/
//! [tracing]: https://docs.rs/tracing/
//! [`tracing-capture`]: https://docs.rs/tracing-capture/

// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use metrics::{
    Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, KeyName, Label, Recorder,
    SetRecorderError, SharedString, Unit,
};
use thread_local::ThreadLocal;
use tracing::field::Value;

use std::{
    collections::HashMap,
    fmt,
    hash::Hash,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, PoisonError, RwLock,
    },
};

#[derive(Debug, Clone, Copy)]
enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

impl MetricKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
        }
    }
}

#[derive(Debug)]
struct MetricMetadata {
    unit: Option<Unit>,
    description: SharedString,
}

impl MetricMetadata {
    const EMPTY: &'static Self = &Self {
        unit: None,
        description: SharedString::const_str(""),
    };
}

#[derive(Debug)]
struct MetricMaps<K, V> {
    counters: HashMap<K, V>,
    gauges: HashMap<K, V>,
    histograms: HashMap<K, V>,
}

impl<K: Eq + Hash, V> MetricMaps<K, V> {
    fn get(&self, kind: MetricKind, key: &K) -> Option<&V> {
        match kind {
            MetricKind::Counter => self.counters.get(key),
            MetricKind::Gauge => self.gauges.get(key),
            MetricKind::Histogram => self.histograms.get(key),
        }
    }

    fn insert(&mut self, kind: MetricKind, key: K, value: V) {
        match kind {
            MetricKind::Counter => self.counters.insert(key, value),
            MetricKind::Gauge => self.gauges.insert(key, value),
            MetricKind::Histogram => self.histograms.insert(key, value),
        };
    }
}

impl<K, V> Default for MetricMaps<K, V> {
    fn default() -> Self {
        Self {
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
        }
    }
}

type MetricMetadataMaps = MetricMaps<String, MetricMetadata>;

struct MetricLabels(Vec<Label>);

impl fmt::Debug for MetricLabels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = formatter.debug_map();
        for label in &self.0 {
            map.entry(&label.key(), &label.value());
        }
        map.finish()
    }
}

#[derive(Debug)]
struct MetricData {
    metadata: Arc<RwLock<MetricMetadataMaps>>,
    name: String,
    labels: MetricLabels,
    value: AtomicU64,
}

impl MetricData {
    fn split_key(key: Key) -> (String, MetricLabels) {
        let (name, labels) = key.into_parts();
        (name.as_str().to_owned(), MetricLabels(labels))
    }

    fn new_counter(metadata: Arc<RwLock<MetricMetadataMaps>>, key: Key) -> Self {
        let (name, labels) = Self::split_key(key);
        Self {
            metadata,
            name,
            labels,
            value: AtomicU64::new(0),
        }
    }

    fn new_gauge(metadata: Arc<RwLock<MetricMetadataMaps>>, key: Key) -> Self {
        let (name, labels) = Self::split_key(key);
        Self {
            metadata,
            name,
            labels,
            value: AtomicU64::new(0.0_f64.to_bits()),
        }
    }

    fn report_metric<T: Value + fmt::Display>(&self, kind: MetricKind, prev_value: T, value: T) {
        let metadata = self.metadata.read().expect("metadata lock poisoned");
        let name = &self.name;
        let metadata = metadata.get(kind, name).unwrap_or(MetricMetadata::EMPTY);
        let unit = metadata.unit.unwrap_or(Unit::Count);
        let (unit_spacing, unit_str) = if matches!(unit, Unit::Count) {
            ("", "")
        } else {
            (" ", unit.as_str())
        };
        let kind = kind.as_str();

        tracing::info!(
            target: env!("CARGO_CRATE_NAME"),
            kind,
            name,
            labels = ?self.labels,
            prev_value,
            value,
            unit = unit.as_str(),
            description = metadata.description.as_ref(),
            "{kind} `{name}` = {value}{unit_spacing}{unit_str}"
        );
    }
}

impl CounterFn for MetricData {
    fn increment(&self, value: u64) {
        let prev_value = self.value.fetch_add(value, Ordering::AcqRel);
        let value = prev_value.wrapping_add(value);
        self.report_metric(MetricKind::Counter, prev_value, value);
    }

    fn absolute(&self, value: u64) {
        let prev_value = self.value.fetch_max(value, Ordering::AcqRel);
        self.report_metric(MetricKind::Counter, prev_value, value);
    }
}

impl GaugeFn for MetricData {
    fn increment(&self, value: f64) {
        let prev_value = loop {
            let result = self
                .value
                .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
                    let current = f64::from_bits(current);
                    Some((current + value).to_bits())
                });
            if let Ok(prev_value) = result {
                break f64::from_bits(prev_value);
            }
        };
        self.report_metric(MetricKind::Gauge, prev_value, prev_value + value);
    }

    fn decrement(&self, value: f64) {
        <Self as GaugeFn>::increment(self, -value);
    }

    fn set(&self, value: f64) {
        let prev_value = self.value.swap(value.to_bits(), Ordering::AcqRel);
        let prev_value = f64::from_bits(prev_value);
        self.report_metric(MetricKind::Gauge, prev_value, value);
    }
}

impl HistogramFn for MetricData {
    fn record(&self, value: f64) {
        let prev_value = self.value.swap(value.to_bits(), Ordering::AcqRel);
        let prev_value = f64::from_bits(prev_value);
        self.report_metric(MetricKind::Histogram, prev_value, value);
    }
}

type MetricDataMaps = MetricMaps<Key, Arc<MetricData>>;

/// Base of the metrics recorder. The `Arc`s and `RwLock`s used within are redundant for
/// per-thread recorder implementation, but since `RwLock`s are not contested, their overhead
/// should be fairly low.
#[derive(Debug, Default)]
struct RecorderBase {
    metadata: Arc<RwLock<MetricMetadataMaps>>,
    metrics: RwLock<MetricDataMaps>,
}

impl RecorderBase {
    fn get_or_insert_metric(&self, kind: MetricKind, key: &Key) -> Arc<MetricData> {
        let metrics = self.metrics.read().expect("metrics lock poisoned");
        if let Some(data) = metrics.get(kind, key) {
            return Arc::clone(data);
        }
        drop(metrics); // to prevent a deadlock on the next line

        let mut metrics = self.metrics.write().expect("metrics lock poisoned");
        if let Some(data) = metrics.get(kind, key) {
            Arc::clone(data)
        } else {
            let metadata = Arc::clone(&self.metadata);
            let metric = Arc::new(match kind {
                MetricKind::Counter => MetricData::new_counter(metadata, key.clone()),
                MetricKind::Gauge | MetricKind::Histogram => {
                    MetricData::new_gauge(metadata, key.clone())
                }
            });
            metrics.insert(kind, key.clone(), Arc::clone(&metric));
            metric
        }
    }

    fn clear(&self) {
        let mut metrics = self.metrics.write().expect("metrics lock poisoned");
        *metrics = MetricDataMaps::default();
        let mut metadata = self.metadata.write().expect("metadata lock poisoned");
        *metadata = MetricMetadataMaps::default();
    }
}

impl Recorder for RecorderBase {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        let mut metadata = self.metadata.write().expect("metadata lock poisoned");
        let key = key.as_str().to_owned();
        metadata
            .counters
            .insert(key, MetricMetadata { unit, description });
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        let mut metadata = self.metadata.write().expect("metadata lock poisoned");
        let key = key.as_str().to_owned();
        metadata
            .gauges
            .insert(key, MetricMetadata { unit, description });
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        let mut metadata = self.metadata.write().expect("metadata lock poisoned");
        let key = key.as_str().to_owned();
        metadata
            .histograms
            .insert(key, MetricMetadata { unit, description });
    }

    fn register_counter(&self, key: &Key) -> Counter {
        let counter = self.get_or_insert_metric(MetricKind::Counter, key);
        Counter::from_arc(counter)
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        let gauge = self.get_or_insert_metric(MetricKind::Gauge, key);
        Gauge::from_arc(gauge)
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        let histogram = self.get_or_insert_metric(MetricKind::Histogram, key);
        Histogram::from_arc(histogram)
    }
}

#[derive(Debug)]
enum Inner {
    Global(RecorderBase),
    PerThread(Box<ThreadLocal<RecorderBase>>),
}

/// Metrics recorder that outputs metric updates as [tracing] events.
///
/// # How to install
///
/// In the debugging use case, you may want to use [`Self::global()`]`.`[`install()`](Self::install()),
/// which will install a recorder that will collect metrics from all threads into a single place.
///
/// For use in tests, you may want to instantiate the recorder with [`Self::per_thread()`] instead.
/// Otherwise, other tests running before and/or in parallel can interfere
/// with the gathered values. This, however, works only if tests and the tested code
/// do not spawn additional threads which report metrics. Interference *may* be acceptable
/// in certain conditions, e.g. if no counters are used and previous values of gauges / histograms
/// are not checked by the test code.
///
/// Finally, if everything else fails, there is [`Self::install_exclusive()`]. It tracks metrics
/// from all threads, and uses a static mutex exclusively locked on each call to ensure
/// that there is no interference.
///
/// Reporter installation will fail on subsequent calls in tests. As long as all tests install
/// the same recorder, this is fine; the installed recorder will provide tracing events for all
/// tests.
///
/// [tracing]: https://docs.rs/tracing/
#[must_use = "Created recorder should be `install()`ed"]
#[derive(Debug)]
pub struct TracingMetricsRecorder {
    inner: Inner,
}

impl TracingMetricsRecorder {
    /// Creates a new recorder that tracks metrics from all threads in a single place (i.e.,
    /// like a real-world metrics recorder).
    pub fn global() -> Self {
        Self {
            inner: Inner::Global(RecorderBase::default()),
        }
    }

    /// Creates a new recorder that tracks metrics from each thread separately. This is useful
    /// for single-threaded tests.
    pub fn per_thread() -> Self {
        Self {
            inner: Inner::PerThread(Box::new(ThreadLocal::new())),
        }
    }

    /// Creates and installs a recorder that tracks metrics from all threads in a single place
    /// (i.e., like [`Self::global()`]), and additionally exclusively locks on each call
    /// so that different runs do not interfere with each other. This can be used
    /// for multithreaded tests.
    ///
    /// # Return value
    ///
    /// Returns a guard that should be held until interference with metrics is a concern.
    /// On drop, the guard will reset the recorder state; it will forget all recorded metrics
    /// and their metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the recorder cannot be installed because another recorder is already
    /// installed as global.
    pub fn install_exclusive() -> Result<RecorderGuard, SetRecorderError> {
        static GLOBAL: Mutex<Option<&'static TracingMetricsRecorder>> = Mutex::new(None);

        let mut guard = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
        // ^ Since we only set the Mutex value once, its value cannot get corrupted.

        let recorder = *guard.get_or_insert_with(|| {
            let global = Box::new(Self::global());
            Box::leak(global)
        });

        metrics::set_recorder(recorder).or_else(|err| {
            let recorder_data_ptr = (recorder as *const Self).cast::<()>();
            let installed_data_ptr = (metrics::recorder() as *const dyn Recorder).cast::<()>();
            if recorder_data_ptr == installed_data_ptr {
                Ok(())
            } else {
                Err(err)
            }
        })?;

        Ok(RecorderGuard { inner: guard })
    }

    /// Installs this recorder as the global recorder.
    ///
    /// # Errors
    ///
    /// Returns an error if the recorder cannot be installed because another recorder is already
    /// installed as global.
    pub fn install(self) -> Result<(), SetRecorderError> {
        metrics::set_boxed_recorder(Box::new(self))
    }

    fn base(&self) -> &RecorderBase {
        match &self.inner {
            Inner::Global(base) => base,
            Inner::PerThread(locals) => locals.get_or_default(),
        }
    }
}

impl Recorder for TracingMetricsRecorder {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.base().describe_counter(key, unit, description);
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.base().describe_gauge(key, unit, description);
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.base().describe_histogram(key, unit, description);
    }

    fn register_counter(&self, key: &Key) -> Counter {
        self.base().register_counter(key)
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        self.base().register_gauge(key)
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        self.base().register_histogram(key)
    }
}

/// Guard returned by [`TracingMetricsRecorder::install_exclusive()`]. Should be held
/// while interference with metrics is a concern (e.g., for a duration of a test).
#[must_use = "Guard must be held to ensure that metrics are not interfered with"]
#[derive(Debug)]
pub struct RecorderGuard {
    inner: MutexGuard<'static, Option<&'static TracingMetricsRecorder>>,
}

impl Drop for RecorderGuard {
    fn drop(&mut self) {
        if let Some(recorder) = *self.inner {
            recorder.base().clear();
        }
    }
}
