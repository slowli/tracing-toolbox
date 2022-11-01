//! Tests for `CapturedSpan` / `CapturedEvent` predicates.

use predicates::{
    constant::always,
    ord::eq,
    prelude::*,
    reflection::{Case, Product},
    str::{ends_with, starts_with},
};
use tracing_core::{
    callsite::DefaultCallsite, field::FieldSet, Kind, Level, LevelFilter, Metadata,
};

use super::*;
use crate::Storage;
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
    let mut storage = Storage::new();
    let span_id = storage.push_span(METADATA, TracedValues::new(), None);
    let span = storage.span(span_id);

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
    let mut storage = Storage::new();
    let span_id = storage.push_span(METADATA, TracedValues::new(), None);
    let span = storage.span(span_id);

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
    let mut storage = Storage::new();
    let span_id = storage.push_span(METADATA, TracedValues::new(), None);
    let span = storage.span(span_id);

    let predicate = name(eq("test_span"));
    assert!(predicate.eval(&span));
    let predicate = name(starts_with("test"));
    assert!(predicate.eval(&span));
    let predicate = name(ends_with("test"));
    assert!(!predicate.eval(&span));
}

#[test]
fn compound_predicates() {
    let mut storage = Storage::new();
    let span_id = storage.push_span(METADATA, TracedValues::new(), None);
    let span = storage.span(span_id);

    let predicate = target("tracing_capture")
        & name(eq("test_span"))
        & level(Level::INFO)
        & field("val", 42_u64);

    assert!(!predicate.eval(&span));
    let case = predicate.find_case(false, &span).unwrap();
    let products: Vec<_> = collect_products(&case);
    assert_eq!(products.len(), 1);
    assert_eq!(products[0].name(), "fields.val");
    assert_eq!(products[0].value().to_string(), "None");

    storage.spans[span_id].values = TracedValues::from_iter([("val", 23_u64.into())]);
    let span = storage.span(span_id);
    let case = predicate.find_case(false, &span).unwrap();
    let products = collect_products(&case);
    assert_eq!(products.len(), 1);
    assert_eq!(products[0].name(), "var");
    assert_eq!(products[0].value().to_string(), "UInt(23)");

    storage.spans[span_id].values = TracedValues::from_iter([("val", 42_u64.into())]);
    let span = storage.span(span_id);
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
    let mut storage = Storage::new();
    let values = TracedValues::from_iter([("val", "str".into())]);
    let span_id = storage.push_span(METADATA, values, None);
    let span = storage.span(span_id);

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
    let mut storage = Storage::new();
    let values = TracedValues::from_iter([
        ("val", 42_i64.into()),
        (
            "message",
            TracedValue::debug(&format_args!("completed computations")),
        ),
    ]);
    let event_id = storage.push_event(EVENT_METADATA, values, None);
    let event = storage.event(event_id);
    let predicate = message(eq("completed computations"));
    assert!(predicate.eval(&event));

    storage.events[event_id].values.remove("message");
    assert!(!predicate.eval(&storage.event(event_id)));
    storage.events[event_id]
        .values
        .insert("message", 555_u64.into());
    assert!(!predicate.eval(&storage.event(event_id)));

    storage.events[event_id]
        .values
        .insert("message", "completed computations".into());
    let event = storage.event(event_id);
    assert!(predicate.eval(&event));
    let predicate =
        message(starts_with("completed")) & level(Level::DEBUG) & target("tracing_capture");
    assert!(predicate.eval(&event));
}

#[test]
fn using_extensions() {
    let mut storage = Storage::new();
    for val in 0_i64..5 {
        let values = TracedValues::from_iter([
            ("val", val.into()),
            (
                "message",
                TracedValue::debug(&format_args!("completed computations")),
            ),
        ]);
        storage.push_event(EVENT_METADATA, values, None);
    }
    let scanner = storage.scan_events();

    let predicate =
        level(LevelFilter::DEBUG) & message(starts_with("completed")) & field("val", 1_i64);
    let event = scanner.single(&predicate);
    assert_eq!(event["val"], 1_i64);
    let event = scanner.first(&field("val", [always()]));
    assert_eq!(event["val"], 0_i64);
    let event = scanner.last(&predicate);
    assert_eq!(event["val"], 1_i64);

    scanner.all(&field("val", [always()]));
    scanner.none(&level(LevelFilter::INFO));
}
