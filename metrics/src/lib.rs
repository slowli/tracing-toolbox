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
        Arc, RwLock,
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

/// Base of the metrics recorder.
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

#[derive(Debug)]
pub struct TracingMetricsRecorder {
    inner: Inner,
}

impl TracingMetricsRecorder {
    pub fn global() -> Self {
        Self {
            inner: Inner::Global(RecorderBase::default()),
        }
    }

    pub fn per_thread() -> Self {
        Self {
            inner: Inner::PerThread(Box::new(ThreadLocal::new())),
        }
    }

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
        self.base().describe_counter(key, unit, description)
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.base().describe_gauge(key, unit, description)
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.base().describe_histogram(key, unit, description)
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
