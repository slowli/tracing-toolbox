//! Helper combinators for predicates.

use predicates::{
    reflection::{Case, PredicateReflection},
    Predicate,
};

use std::fmt;

/// Boolean "and" combinator for predicates. Produced by the bitwise and (`&`) operator
/// on the base predicates from this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct And<T, U> {
    first: T,
    second: U,
}

impl<T: PredicateReflection, U: PredicateReflection> And<T, U> {
    pub(crate) fn new(first: T, second: U) -> Self {
        Self { first, second }
    }
}

impl<T: fmt::Display, U: fmt::Display> fmt::Display for And<T, U> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "({} && {})", self.first, self.second)
    }
}

impl<T: PredicateReflection, U: PredicateReflection> PredicateReflection for And<T, U> {}

impl<T, U, Item: ?Sized> Predicate<Item> for And<T, U>
where
    T: Predicate<Item>,
    U: Predicate<Item>,
{
    fn eval(&self, variable: &Item) -> bool {
        self.first.eval(variable) && self.second.eval(variable)
    }

    fn find_case(&self, expected: bool, variable: &Item) -> Option<Case<'_>> {
        if expected {
            // We need both child cases.
            let first = self.first.find_case(expected, variable)?;
            let second = self.second.find_case(expected, variable)?;
            let case = Case::new(Some(self), expected)
                .add_child(first)
                .add_child(second);
            Some(case)
        } else {
            // Return either of cases if present.
            if let Some(child) = self.first.find_case(expected, variable) {
                Some(Case::new(Some(self), expected).add_child(child))
            } else {
                self.second
                    .find_case(expected, variable)
                    .map(|child| Case::new(Some(self), expected).add_child(child))
            }
        }
    }
}

/// Boolean "or" combinator for predicates. Produced by the bitwise or (`|`) operator
/// on the base predicates from this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Or<T, U> {
    first: T,
    second: U,
}

impl<T: PredicateReflection, U: PredicateReflection> Or<T, U> {
    pub(crate) fn new(first: T, second: U) -> Self {
        Self { first, second }
    }
}

impl<T: fmt::Display, U: fmt::Display> fmt::Display for Or<T, U> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "({} || {})", self.first, self.second)
    }
}

impl<T: PredicateReflection, U: PredicateReflection> PredicateReflection for Or<T, U> {}

impl<T, U, Item: ?Sized> Predicate<Item> for Or<T, U>
where
    T: Predicate<Item>,
    U: Predicate<Item>,
{
    fn eval(&self, variable: &Item) -> bool {
        self.first.eval(variable) || self.second.eval(variable)
    }

    fn find_case(&self, expected: bool, variable: &Item) -> Option<Case<'_>> {
        if expected {
            // Return either of cases if present.
            if let Some(child) = self.first.find_case(expected, variable) {
                Some(Case::new(Some(self), expected).add_child(child))
            } else {
                self.second
                    .find_case(expected, variable)
                    .map(|child| Case::new(Some(self), expected).add_child(child))
            }
        } else {
            // We need both child cases.
            let first = self.first.find_case(expected, variable)?;
            let second = self.second.find_case(expected, variable)?;
            let case = Case::new(Some(self), expected)
                .add_child(first)
                .add_child(second);
            Some(case)
        }
    }
}

macro_rules! impl_bool_ops {
    ($name:ident <$($ty_var:ident),+>) => {
        impl<Rhs, $($ty_var,)+> core::ops::BitAnd<Rhs> for $name<$($ty_var,)+>
        where
            Self: predicates::reflection::PredicateReflection,
            Rhs: predicates::reflection::PredicateReflection,
        {
            type Output = $crate::predicates::And<Self, Rhs>;

            fn bitand(self, rhs: Rhs) -> Self::Output {
                $crate::predicates::And::new(self, rhs)
            }
        }

        impl<Rhs, $($ty_var,)+> core::ops::BitOr<Rhs> for $name<$($ty_var,)+>
        where
            Self: predicates::reflection::PredicateReflection,
            Rhs: predicates::reflection::PredicateReflection,
        {
            type Output = $crate::predicates::Or<Self, Rhs>;

            fn bitor(self, rhs: Rhs) -> Self::Output {
                $crate::predicates::Or::new(self, rhs)
            }
        }
    };
}

impl_bool_ops!(And<T, U>);
impl_bool_ops!(Or<T, U>);
