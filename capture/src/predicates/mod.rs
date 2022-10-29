//! [`Predicate`]s for [`CapturedSpan`]s and [`CapturedEvent`]s.

#![allow(missing_docs)] // FIXME

use predicates::{
    reflection::{Case, PredicateReflection, Product},
    Predicate,
};
use tracing_core::{Level, LevelFilter};

use std::fmt;

#[macro_use]
mod combinators;

pub use self::combinators::{And, Or};

use crate::{CapturedEvent, CapturedSpan};
use tracing_tunnel::TracedValue;

pub trait IntoLevelPredicate {
    type Predicate: Predicate<Level>;

    fn into_predicate(self) -> Self::Predicate;
}

impl<P: Predicate<Level>> IntoLevelPredicate for [P; 1] {
    type Predicate = P;

    fn into_predicate(self) -> Self::Predicate {
        self.into_iter().next().unwrap()
    }
}

impl IntoLevelPredicate for Level {
    type Predicate = predicates::ord::EqPredicate<Level>;

    fn into_predicate(self) -> Self::Predicate {
        predicates::ord::eq(self)
    }
}

impl IntoLevelPredicate for LevelFilter {
    type Predicate = predicates::ord::OrdPredicate<Level>;

    fn into_predicate(self) -> Self::Predicate {
        self.into_level()
            .map_or_else(|| predicates::ord::lt(Level::ERROR), predicates::ord::le)
    }
}

pub fn level<P: IntoLevelPredicate>(matches: P) -> LevelPredicate<P::Predicate> {
    LevelPredicate {
        matches: matches.into_predicate(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelPredicate<P> {
    matches: P,
}

impl<P: Predicate<Level>> fmt::Display for LevelPredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "level({})", self.matches)
    }
}

impl<P: Predicate<Level>> PredicateReflection for LevelPredicate<P> {}

impl<P: Predicate<Level>> Predicate<CapturedSpan> for LevelPredicate<P> {
    fn eval(&self, variable: &CapturedSpan) -> bool {
        self.matches.eval(variable.metadata().level())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().level())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

impl<P: Predicate<Level>> Predicate<CapturedEvent> for LevelPredicate<P> {
    fn eval(&self, variable: &CapturedEvent) -> bool {
        self.matches.eval(variable.metadata().level())
    }

    fn find_case(&self, expected: bool, variable: &CapturedEvent) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().level())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

pub const fn name<P: Predicate<str>>(matches: P) -> NamePredicate<P> {
    NamePredicate { matches }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamePredicate<P> {
    matches: P,
}

impl<P: Predicate<str>> fmt::Display for NamePredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "name({})", self.matches)
    }
}

impl<P: Predicate<str>> PredicateReflection for NamePredicate<P> {}

impl<P: Predicate<str>> Predicate<CapturedSpan> for NamePredicate<P> {
    fn eval(&self, variable: &CapturedSpan) -> bool {
        self.matches.eval(variable.metadata().name())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().name())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

pub trait IntoTargetPredicate {
    type Predicate: Predicate<str>;

    fn into_predicate(self) -> Self::Predicate;
}

impl<P: Predicate<str>> IntoTargetPredicate for [P; 1] {
    type Predicate = P;

    fn into_predicate(self) -> Self::Predicate {
        self.into_iter().next().unwrap()
    }
}

impl IntoTargetPredicate for &str {
    type Predicate = predicates::str::RegexPredicate;

    fn into_predicate(self) -> Self::Predicate {
        predicates::str::is_match(format!("^{self}($|::)")).unwrap()
    }
}

pub fn target<P: IntoTargetPredicate>(matches: P) -> TargetPredicate<P::Predicate> {
    TargetPredicate {
        matches: matches.into_predicate(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPredicate<P> {
    matches: P,
}

impl<P: Predicate<str>> fmt::Display for TargetPredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "target({})", self.matches)
    }
}

impl<P: Predicate<str>> PredicateReflection for TargetPredicate<P> {}

impl<P: Predicate<str>> Predicate<CapturedSpan> for TargetPredicate<P> {
    fn eval(&self, variable: &CapturedSpan) -> bool {
        self.matches.eval(variable.metadata().target())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().target())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

impl<P: Predicate<str>> Predicate<CapturedEvent> for TargetPredicate<P> {
    fn eval(&self, variable: &CapturedEvent) -> bool {
        self.matches.eval(variable.metadata().target())
    }

    fn find_case(&self, expected: bool, variable: &CapturedEvent) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().target())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

pub trait IntoFieldPredicate {
    type Predicate: Predicate<TracedValue>;

    fn into_predicate(self) -> Self::Predicate;
}

impl<P: Predicate<TracedValue>> IntoFieldPredicate for [P; 1] {
    type Predicate = P;

    fn into_predicate(self) -> Self::Predicate {
        self.into_iter().next().unwrap()
    }
}

macro_rules! impl_into_field_predicate {
    ($($ty:ty),+) => {
        $(
        impl IntoFieldPredicate for $ty {
            type Predicate = EquivPredicate<Self>;

            fn into_predicate(self) -> Self::Predicate {
                equiv(self)
            }
        }
        )+
    };
}

// FIXME: add `&str`
impl_into_field_predicate!(bool, i64, i128, u64, u128, f64);

pub fn field<P: IntoFieldPredicate>(
    name: &'static str,
    matches: P,
) -> FieldPredicate<P::Predicate> {
    FieldPredicate {
        name,
        matches: matches.into_predicate(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldPredicate<P> {
    name: &'static str,
    matches: P,
}

impl<P: Predicate<TracedValue>> fmt::Display for FieldPredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "fields.{}({})", self.name, self.matches)
    }
}

impl<P: Predicate<TracedValue>> PredicateReflection for FieldPredicate<P> {}

macro_rules! impl_predicate_for_field {
    ($ty:ty) => {
        impl<P: Predicate<TracedValue>> Predicate<$ty> for FieldPredicate<P> {
            fn eval(&self, variable: &$ty) -> bool {
                variable
                    .value(self.name)
                    .map_or(false, |value| self.matches.eval(value))
            }

            fn find_case(&self, expected: bool, variable: &$ty) -> Option<Case<'_>> {
                let value = if let Some(value) = variable.value(self.name) {
                    value
                } else {
                    return if expected {
                        None // was expecting a variable, but there is none
                    } else {
                        let product = Product::new(format!("fields.{}", self.name), "None");
                        Some(Case::new(Some(self), expected).add_product(product))
                    };
                };

                let child = self.matches.find_case(expected, value)?;
                Some(Case::new(Some(self), expected).add_child(child))
            }
        }
    };
}

impl_predicate_for_field!(CapturedSpan);
impl_predicate_for_field!(CapturedEvent);

pub fn equiv<V: PartialEq<TracedValue>>(value: V) -> EquivPredicate<V> {
    EquivPredicate { value }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquivPredicate<V> {
    value: V,
}

impl<V: fmt::Debug> fmt::Display for EquivPredicate<V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "var ~= {:?}", self.value)
    }
}

impl<V: fmt::Debug> PredicateReflection for EquivPredicate<V> {}

impl<V: fmt::Debug + PartialEq<TracedValue>> Predicate<TracedValue> for EquivPredicate<V> {
    fn eval(&self, variable: &TracedValue) -> bool {
        self.value == *variable
    }

    fn find_case(&self, expected: bool, variable: &TracedValue) -> Option<Case<'_>> {
        if self.eval(variable) == expected {
            let product = Product::new("var", format!("{variable:?}"));
            Some(Case::new(Some(self), expected).add_product(product))
        } else {
            None
        }
    }
}

pub fn into_fn<Item>(predicate: impl Predicate<Item>) -> impl Fn(&Item) -> bool {
    move |variable| predicate.eval(variable)
}

impl_bool_ops!(TargetPredicate<P>);
impl_bool_ops!(NamePredicate<P>);
impl_bool_ops!(LevelPredicate<P>);
impl_bool_ops!(FieldPredicate<P>);

#[cfg(test)]
mod tests {
    use predicates::{
        ord::eq,
        prelude::*,
        str::{ends_with, starts_with},
    };
    use tracing_core::{callsite::DefaultCallsite, field::FieldSet, Kind, Metadata};

    use super::*;
    use crate::SpanStats;
    use tracing_tunnel::TracedValues;

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
}
