//! Helper types and functionality.

use metrics::{CounterFn, GaugeFn, HistogramFn, Key, Label, SharedString, Unit};
use tracing::field::{self, Value};

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
pub(crate) enum MetricKind {
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
pub(crate) struct MetricMetadata {
    unit: Option<Unit>,
    description: SharedString,
}

impl MetricMetadata {
    const EMPTY: &'static Self = &Self {
        unit: None,
        description: SharedString::const_str(""),
    };

    pub fn new(unit: Option<Unit>, description: SharedString) -> Self {
        Self { unit, description }
    }
}

#[derive(Debug)]
pub(crate) struct MetricMaps<K, V> {
    counters: HashMap<K, V>,
    gauges: HashMap<K, V>,
    histograms: HashMap<K, V>,
}

pub(crate) type MetricMetadataMaps = MetricMaps<String, MetricMetadata>;

impl<K: Eq + Hash, V> MetricMaps<K, V> {
    pub fn get(&self, kind: MetricKind, key: &K) -> Option<&V> {
        match kind {
            MetricKind::Counter => self.counters.get(key),
            MetricKind::Gauge => self.gauges.get(key),
            MetricKind::Histogram => self.histograms.get(key),
        }
    }

    pub fn insert(&mut self, kind: MetricKind, key: K, value: V) {
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

impl fmt::Display for MetricLabels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

#[derive(Debug)]
pub(crate) struct MetricData {
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

    pub fn new_counter(metadata: Arc<RwLock<MetricMetadataMaps>>, key: Key) -> Self {
        let (name, labels) = Self::split_key(key);
        Self {
            metadata,
            name,
            labels,
            value: AtomicU64::new(0),
        }
    }

    pub fn new_gauge(metadata: Arc<RwLock<MetricMetadataMaps>>, key: Key) -> Self {
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
        let unit = metadata.unit;
        let (unit_spacing, unit_str) = match &unit {
            None | Some(Unit::Count) => ("", ""),
            Some(unit) => (" ", unit.as_str()),
        };
        let kind = kind.as_str();
        let description = metadata.description.as_ref();
        let labels_str: &dyn fmt::Display = if self.labels.0.is_empty() {
            &""
        } else {
            &self.labels
        };

        tracing::info!(
            target: env!("CARGO_CRATE_NAME"),
            kind,
            name,
            labels = if self.labels.0.is_empty() {
                None
            } else {
                Some(field::debug(&self.labels))
            },
            prev_value,
            value,
            unit = unit.as_ref().map(Unit::as_str),
            description = if description.is_empty() {
                None
            } else {
                Some(description)
            },
            "{kind} {name}{labels_str} = {value}{unit_spacing}{unit_str}"
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
