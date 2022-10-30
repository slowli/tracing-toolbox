//! Tests for `CapturedSpan` / `CapturedEvent` predicates.

use predicates::{
    ord::eq,
    prelude::*,
    reflection::{Case, Product},
    str::{ends_with, starts_with},
};
use tracing_core::{
    callsite::DefaultCallsite, field::FieldSet, Kind, Level, LevelFilter, Metadata,
};

use super::*;
use crate::SpanStats;
use tracing_tunnel::{TracedValue, TracedValues};

static SITE: DefaultCallsite = DefaultCallsite::new(METADATA);
static METADATA: &Metadata<'static> = &Metadata::new(
    "test_span",
    "tracing_capture::predicate",
    Level::INFO,
    Some("predicate.rs"),
    Some(42),
    Some("predicate"),
    FieldSet::new(&["val"], tracing_core::identify_callsite!(&SITE)),
    Kind::SPAN,
);
static EVENT_METADATA: &Metadata<'static> = &Metadata::new(
    "event at tracing_capture/predicates.rs:42",
    "tracing_capture::predicate",
    Level::DEBUG,
    Some("predicate.rs"),
    Some(42),
    Some("predicate"),
    FieldSet::new(&["val", "message"], tracing_core::identify_callsite!(&SITE)),
    Kind::EVENT,
);

#[test]
fn level_predicates() {
    let span = CapturedSpan {
        metadata: METADATA,
        values: TracedValues::new(),
        stats: SpanStats::default(),
        events: vec![],
    };

    let predicate = level(Level::INFO);
    assert!(predicate.eval(&span));
    let predicate = level(Level::DEBUG);
    assert!(!predicate.eval(&span));
    let predicate = level(Level::WARN);
    assert!(!predicate.eval(&span));

    let predicate = level(LevelFilter::INFO);
    assert!(predicate.eval(&span));
    let predicate = level(LevelFilter::DEBUG);
    assert!(predicate.eval(&span));
    let predicate = level(LevelFilter::WARN);
    assert!(!predicate.eval(&span));
    let predicate = level(LevelFilter::OFF);
    assert!(!predicate.eval(&span));
}

#[test]
fn target_predicates() {
    let span = CapturedSpan {
        metadata: METADATA,
        values: TracedValues::new(),
        stats: SpanStats::default(),
        events: vec![],
    };

    let predicate = target("tracing_capture");
    assert!(predicate.eval(&span));
    let predicate = target("tracing");
    assert!(!predicate.eval(&span));
    let predicate = target("tracing_capture::predicate");
    assert!(predicate.eval(&span));
    let predicate = target("tracing_capture::pred");
    assert!(!predicate.eval(&span));
}

#[test]
fn name_predicates() {
    let span = CapturedSpan {
        metadata: METADATA,
        values: TracedValues::new(),
        stats: SpanStats::default(),
        events: vec![],
    };

    let predicate = name(eq("test_span"));
    assert!(predicate.eval(&span));
    let predicate = name(starts_with("test"));
    assert!(predicate.eval(&span));
    let predicate = name(ends_with("test"));
    assert!(!predicate.eval(&span));
}

#[test]
fn compound_predicates() {
    let predicate = target("tracing_capture")
        & name(eq("test_span"))
        & level(Level::INFO)
        & field("val", 42_u64);

    let mut span = CapturedSpan {
        metadata: METADATA,
        values: TracedValues::new(),
        stats: SpanStats::default(),
        events: vec![],
    };
    assert!(!predicate.eval(&span));
    let case = predicate.find_case(false, &span).unwrap();
    let products: Vec<_> = collect_products(&case);
    assert_eq!(products.len(), 1);
    assert_eq!(products[0].name(), "fields.val");
    assert_eq!(products[0].value().to_string(), "None");

    span.values = TracedValues::from_iter([("val", 23_u64.into())]);
    let case = predicate.find_case(false, &span).unwrap();
    let products = collect_products(&case);
    assert_eq!(products.len(), 1);
    assert_eq!(products[0].name(), "val");
    assert_eq!(products[0].value().to_string(), "UInt(23)");

    span.values = TracedValues::from_iter([("val", 42_u64.into())]);
    let eval = predicate.eval(&span);
    assert!(eval);
}

fn collect_products<'r>(case: &'r Case<'_>) -> Vec<&'r Product> {
    let mut cases = vec![case];
    let mut products = vec![];
    while !cases.is_empty() {
        products.extend(cases.iter().copied().flat_map(Case::products));
        cases = cases.into_iter().flat_map(Case::children).collect();
    }
    products
}

#[test]
fn compound_predicates_combining_and_or() {
    let span = CapturedSpan {
        metadata: METADATA,
        values: TracedValues::from_iter([("val", "str".into())]),
        stats: SpanStats::default(),
        events: vec![],
    };

    let predicate = (target("tracing_capture") | field("val", 23_u64)) & level(Level::INFO);
    assert!(predicate.eval(&span));
    let case = predicate.find_case(true, &span).unwrap();
    let products = collect_products(&case);
    assert_eq!(products.len(), 2);
    let level_value = products[0].value().to_string();
    assert!(level_value.contains("Info"), "{level_value}");
    assert_eq!(
        products[1].value().to_string(),
        "tracing_capture::predicate"
    );

    let predicate = (target("tracing") | field("val", 23_u64)) & level(Level::INFO);
    assert!(!predicate.eval(&span));
    let case = predicate.find_case(false, &span).unwrap();
    let products = collect_products(&case);
    assert_eq!(products.len(), 2);
    assert_eq!(
        products[0].value().to_string(),
        "tracing_capture::predicate"
    );
    assert_eq!(products[1].value().to_string(), "String(\"str\")");
}

#[test]
fn message_predicates() {
    let mut event = CapturedEvent {
        metadata: EVENT_METADATA,
        values: TracedValues::from_iter([
            ("val", 42_i64.into()),
            (
                "message",
                TracedValue::debug(&format_args!("completed computations")),
            ),
        ]),
    };
    let predicate = message(eq("completed computations"));
    assert!(predicate.eval(&event));

    event.values.remove("message");
    assert!(!predicate.eval(&event));
    event.values.insert("message", 555_u64.into());
    assert!(!predicate.eval(&event));
    event
        .values
        .insert("message", "completed computations".into());
    assert!(predicate.eval(&event));

    let predicate =
        message(starts_with("completed")) & level(Level::DEBUG) & target("tracing_capture");
    assert!(predicate.eval(&event));
}
