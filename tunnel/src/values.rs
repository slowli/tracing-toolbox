//! `TracedValues` and closely related types.

use serde::{
    de::{MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};
use tracing_core::{
    field::{Field, ValueSet, Visit},
    span::Record,
    Event,
};

use core::{fmt, mem, ops, slice};

use crate::{
    alloc::{vec, String, Vec},
    TracedValue,
};

/// Collection of named [`TracedValue`]s.
///
/// Functionally this collection is similar to a `HashMap<S, TracedValue>`,
/// with the key being that the order of [iteration](Self::iter()) is the insertion order.
/// If a value is updated, including via [`Extend`] etc., it preserves its old placement.
#[derive(Clone)]
pub struct TracedValues<S> {
    inner: Vec<(S, TracedValue)>,
}

impl<S> Default for TracedValues<S> {
    fn default() -> Self {
        Self { inner: Vec::new() }
    }
}

impl<S: AsRef<str>> fmt::Debug for TracedValues<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = formatter.debug_map();
        for (key, value) in &self.inner {
            map.entry(&key.as_ref(), value);
        }
        map.finish()
    }
}

impl<S: From<&'static str> + AsRef<str>> TracedValues<S> {
    /// Creates traced values from the specified value set.
    pub fn from_values(values: &ValueSet<'_>) -> Self {
        let mut visitor = TracedValueVisitor {
            values: Self::default(),
        };
        values.record(&mut visitor);
        visitor.values
    }

    /// Creates traced values from the specified record.
    pub fn from_record(values: &Record<'_>) -> Self {
        let mut visitor = TracedValueVisitor {
            values: Self::default(),
        };
        values.record(&mut visitor);
        visitor.values
    }

    /// Creates traced values from the values in the specified event.
    pub fn from_event(event: &Event<'_>) -> Self {
        let mut visitor = TracedValueVisitor {
            values: Self::default(),
        };
        event.record(&mut visitor);
        visitor.values
    }
}

impl<S: AsRef<str>> TracedValues<S> {
    /// Creates new empty values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of stored values.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Checks whether this collection of values is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns the value with the specified name, or `None` if it not set.
    pub fn get(&self, key: &str) -> Option<&TracedValue> {
        self.inner.iter().find_map(|(existing_key, value)| {
            if existing_key.as_ref() == key {
                Some(value)
            } else {
                None
            }
        })
    }

    /// Iterates over the contained name-value pairs.
    pub fn iter(&self) -> TracedValuesIter<'_, S> {
        TracedValuesIter {
            inner: self.inner.iter(),
        }
    }

    /// Inserts a value with the specified name. If a value with the same name was present
    /// previously, it is overwritten.
    pub fn insert(&mut self, key: S, value: TracedValue) -> Option<TracedValue> {
        let position = self
            .inner
            .iter()
            .position(|(existing_key, _)| existing_key.as_ref() == key.as_ref());
        if let Some(position) = position {
            let place = &mut self.inner[position].1;
            Some(mem::replace(place, value))
        } else {
            self.inner.push((key, value));
            None
        }
    }
}

impl<S: AsRef<str>> ops::Index<&str> for TracedValues<S> {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.get(index)
            .unwrap_or_else(|| panic!("value `{index}` is not defined"))
    }
}

impl<S: AsRef<str>> FromIterator<(S, TracedValue)> for TracedValues<S> {
    fn from_iter<I: IntoIterator<Item = (S, TracedValue)>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let mut this = Self::new();
        this.extend(iter);
        this
    }
}

impl<S: AsRef<str>> Extend<(S, TracedValue)> for TracedValues<S> {
    fn extend<I: IntoIterator<Item = (S, TracedValue)>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.inner.reserve(iter.size_hint().0);
        for (name, value) in iter {
            self.insert(name, value);
        }
    }
}

impl<S> IntoIterator for TracedValues<S> {
    type Item = (S, TracedValue);
    type IntoIter = vec::IntoIter<(S, TracedValue)>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

/// Iterator over name-value references returned from [`TracedValues::iter()`].
#[derive(Debug)]
pub struct TracedValuesIter<'a, S> {
    inner: slice::Iter<'a, (S, TracedValue)>,
}

impl<'a, S: AsRef<str>> Iterator for TracedValuesIter<'a, S> {
    type Item = (&'a str, &'a TracedValue);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|(name, value)| (name.as_ref(), value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a, S: AsRef<str>> DoubleEndedIterator for TracedValuesIter<'a, S> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner
            .next_back()
            .map(|(name, value)| (name.as_ref(), value))
    }
}

impl<'a, S: AsRef<str>> ExactSizeIterator for TracedValuesIter<'a, S> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<'a, S: AsRef<str>> IntoIterator for &'a TracedValues<S> {
    type Item = (&'a str, &'a TracedValue);
    type IntoIter = TracedValuesIter<'a, S>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<S: AsRef<str>> Serialize for TracedValues<S> {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (name, value) in self {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for TracedValues<String> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MapVisitor;

        impl<'v> Visitor<'v> for MapVisitor {
            type Value = TracedValues<String>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("map of name-value entries")
            }

            fn visit_map<A: MapAccess<'v>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = TracedValues {
                    inner: Vec::with_capacity(map.size_hint().unwrap_or(0)),
                };
                while let Some((name, value)) = map.next_entry()? {
                    values.insert(name, value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_map(MapVisitor)
    }
}

struct TracedValueVisitor<S> {
    values: TracedValues<S>,
}

impl<S: AsRef<str>> fmt::Debug for TracedValueVisitor<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValueVisitor")
            .field("values", &self.values)
            .finish()
    }
}

impl<S: From<&'static str> + AsRef<str>> Visit for TracedValueVisitor<S> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values.insert(field.name().into(), value.into());
    }

    #[cfg(feature = "std")]
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.values
            .insert(field.name().into(), TracedValue::error(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.values
            .insert(field.name().into(), TracedValue::debug(value));
    }
}
