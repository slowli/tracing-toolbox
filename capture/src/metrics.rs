//! Support of [`metrics`] events emitted by a [`tracing-metrics-recorder`].
//!
//! Provided that a `TracingMetricsRecorder` is installed as a metrics recorder,
//! a [`MetricUpdateEvent`] event is emitted with [`MetricUpdateEvent::TARGET`]
//! on the `INFO` level each time one of the "main" `metrics` macros is called
//! (`counter!`, `gauge!`, `histogram!` etc.). Just like other tracing events, these events
//! are attached to the currently active span(s).
//!
//! See `tracing-metrics-recorder` docs for the examples of usage.
//!
//! [`metrics`]: https://docs.rs/metrics/
//! [`tracing-metrics-recorder`]: https://docs.rs/tracing-metrics-recorder/

use std::collections::HashMap;

use crate::CapturedEvent;
use tracing_tunnel::TracedValue;

/// Kind of a metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKind {
    /// Counter metric.
    Counter,
    /// Gauge metric.
    Gauge,
    /// Histogram metric.
    Histogram,
}

impl MetricKind {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "counter" => Some(Self::Counter),
            "gauge" => Some(Self::Gauge),
            "histogram" => Some(Self::Histogram),
            _ => None,
        }
    }
}

/// Information about a metric.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metric<'a> {
    /// Metric kind (a counter, gauge or histogram).
    pub kind: MetricKind,
    /// Name of the metric specified in its `counter!`, `gauge!` or `histogram!` macro.
    pub name: &'a str,
    /// Metric labels specified in its `counter!`, `gauge!` or `histogram!` macro.
    pub labels: HashMap<&'a str, &'a str>,
    /// String representation of the measurement unit of the metric, specified in its
    /// `describe_*` macro.
    pub unit: &'a str,
    /// Human-readable metric description specified in its `describe_*` macro.
    pub description: &'a str,
}

/// Update event for a metric. Can be parsed from a [`CapturedEvent`] using
/// its [`as_metric_update()`] method.
///
/// Metric update events are emitted using [`Self::TARGET`] on the `INFO` level, which can be used
/// to filter captured events.
///
/// [`as_metric_update()`]: CapturedEvent::as_metric_update()
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MetricUpdateEvent<'a> {
    /// Information about the updated metric.
    pub metric: Metric<'a>,
    /// The metric value after the update. Counter metrics have unsigned integer values,
    /// while gauges and histograms have floating-point values.
    pub value: &'a TracedValue,
    /// The metric value before the update.
    pub prev_value: &'a TracedValue,
}

impl<'a> MetricUpdateEvent<'a> {
    /// The target used by metric update events: `tracing_metrics_recorder`.
    pub const TARGET: &'static str = "tracing_metrics_recorder";

    pub(crate) fn new(event: &CapturedEvent<'a>) -> Option<Self> {
        if event.metadata().target() != Self::TARGET {
            return None;
        }
        let metric = Metric {
            kind: MetricKind::from_str(event.value("kind")?.as_str()?)?,
            name: event.value("name")?.as_str()?,
            unit: Self::get_optional_str(event.value("unit"))?,
            description: Self::get_optional_str(event.value("description"))?,
            labels: Self::parse_labels(event.value("labels"))?,
        };
        let value = event.value("value")?;
        let prev_value = event.value("prev_value")?;
        Some(Self {
            metric,
            value,
            prev_value,
        })
    }

    fn get_optional_str(value: Option<&TracedValue>) -> Option<&str> {
        if let Some(value) = value {
            value.as_str()
        } else {
            Some("")
        }
    }

    /// Parses debug presentation of labels, such as `{"stage": "init", "location": "UK"}`.
    fn parse_labels(labels: Option<&TracedValue>) -> Option<HashMap<&str, &str>> {
        if let Some(labels) = labels {
            Self::parse_labels_inner(labels.as_debug_str()?)
        } else {
            Some(HashMap::new())
        }
    }

    fn parse_labels_inner(labels: &str) -> Option<HashMap<&str, &str>> {
        if labels.contains('\\') {
            // We don't support escape sequences yet
            return Some(HashMap::new());
        }

        let labels = labels.trim();
        if !labels.starts_with('{') || !labels.ends_with('}') {
            return None;
        }
        let mut labels = labels[1..labels.len() - 1].trim();

        let mut label_map = HashMap::new();
        while !labels.is_empty() {
            let key = Self::read_str(&mut labels)?;
            if !labels.starts_with(':') {
                return None;
            }
            labels = labels[1..].trim_start(); // Trim `:` and following whitespace
            let value = Self::read_str(&mut labels)?;

            if !labels.is_empty() {
                if !labels.starts_with(',') {
                    return None;
                }
                labels = labels[1..].trim_start(); // Trim `,` and following whitespace
            }
            label_map.insert(key, value);
        }
        Some(label_map)
    }

    fn read_str<'r>(labels: &mut &'r str) -> Option<&'r str> {
        if !labels.starts_with('"') {
            return None;
        }
        *labels = &labels[1..];
        let str_end = labels.find('"')?;
        let str = &labels[..str_end];
        *labels = labels[(str_end + 1)..].trim_start();
        Some(str)
    }
}

// FIXME: self-contained tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_labels() {
        let labels = MetricUpdateEvent::parse_labels_inner("{}").unwrap();
        assert!(labels.is_empty());
        let labels = MetricUpdateEvent::parse_labels_inner("{  }").unwrap();
        assert!(labels.is_empty());

        let single_label_variants = [
            r#"{"stage": "init"}"#,
            r#"{"stage":"init"}"#,
            r#"{"stage" : "init" }"#,
            r#"{ "stage": "init", }"#,
        ];
        for labels in single_label_variants {
            let labels = MetricUpdateEvent::parse_labels_inner(labels).unwrap();
            assert_eq!(labels.len(), 1);
            assert_eq!(labels["stage"], "init");
        }

        let multi_label_variants = [
            r#"{"stage": "init", "location": "UK"}"#,
            r#"{"stage":"init","location":"UK"}"#,
            r#"{"stage" : "init"  , "location"  : "UK"  }"#,
            r#"{ "stage": "init", "location": "UK", }"#,
        ];
        for labels in multi_label_variants {
            let labels = MetricUpdateEvent::parse_labels_inner(labels).unwrap();
            assert_eq!(labels.len(), 2);
            assert_eq!(labels["stage"], "init");
            assert_eq!(labels["location"], "UK");
        }
    }
}
