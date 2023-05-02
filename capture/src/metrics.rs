//! Metric support.

#![allow(missing_docs)] // FIXME

use std::collections::HashMap;

use crate::CapturedEvent;
use tracing_tunnel::TracedValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKind {
    Counter,
    Gauge,
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

#[derive(Debug)]
#[non_exhaustive]
pub struct Metric<'a> {
    pub kind: MetricKind,
    pub name: &'a str,
    pub labels: HashMap<&'a str, &'a str>,
    pub unit: &'a str,
    pub description: &'a str,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct MetricUpdateEvent<'a> {
    pub metric: Metric<'a>,
    pub value: &'a TracedValue,
    pub prev_value: &'a TracedValue,
}

impl<'a> MetricUpdateEvent<'a> {
    pub(crate) fn new(event: &CapturedEvent<'a>) -> Option<Self> {
        const EXPECTED_TARGET: &str = "tracing_metrics_recorder";

        if event.metadata().target() != EXPECTED_TARGET {
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
