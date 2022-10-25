//! Helpers to (de)serialize some parts of `TracingEvent`s.

#[cfg(feature = "consumer")]
pub(crate) mod span_id {
    use serde::{
        de::{Error as DeError, Visitor},
        Deserializer, Serializer,
    };
    use tracing_core::span::Id;

    use std::fmt;

    pub fn serialize<S: Serializer>(id: &Id, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(id.into_u64())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Id, D::Error> {
        struct NumberVisitor;

        impl Visitor<'_> for NumberVisitor {
            type Value = Id;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("numeric span ID")
            }

            fn visit_u64<E: DeError>(self, value: u64) -> Result<Self::Value, E> {
                if value == 0 {
                    Err(E::custom("span IDs must be positive"))
                } else {
                    Ok(Id::from_u64(value))
                }
            }
        }

        deserializer.deserialize_u64(NumberVisitor)
    }
}

pub(crate) mod tuples {
    use serde::{
        de::{MapAccess, Visitor},
        ser::SerializeMap,
        Deserialize, Deserializer, Serialize, Serializer,
    };

    use std::{fmt, marker::PhantomData};

    pub fn serialize<S: Serializer, T: Serialize>(
        tuples: &[(String, T)],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(tuples.len()))?;
        for (name, value) in tuples {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<Vec<(String, T)>, D::Error> {
        struct MapVisitor<T> {
            _ty: PhantomData<T>,
        }

        impl<T> Default for MapVisitor<T> {
            fn default() -> Self {
                Self { _ty: PhantomData }
            }
        }

        impl<'de, T: Deserialize<'de>> Visitor<'de> for MapVisitor<T> {
            type Value = Vec<(String, T)>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("map of name-value pairs")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = map.size_hint().map_or_else(Vec::new, Vec::with_capacity);
                while let Some(tuple) = map.next_entry::<String, T>()? {
                    entries.push(tuple);
                }
                Ok(entries)
            }
        }

        deserializer.deserialize_map(MapVisitor::<T>::default())
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use tracing_core::span::Id;

    use super::{span_id, tuples};

    #[derive(Debug, Serialize, Deserialize)]
    struct IdWrapper {
        #[serde(with = "span_id")]
        id: Id,
    }

    #[test]
    fn span_serialization() {
        let id = Id::from_u64(42);
        let wrapper = IdWrapper { id: id.clone() };
        let serialized = serde_json::to_value(wrapper).unwrap();
        assert_eq!(serialized, serde_json::json!({ "id": 42 }));

        let restored: IdWrapper = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored.id, id);
    }

    #[test]
    fn zero_span_deserialization() {
        let serialized = serde_json::json!({ "id": 0 });
        let err = serde_json::from_value::<IdWrapper>(serialized).unwrap_err();
        let err = err.to_string();
        assert!(err.contains("span IDs must be positive"), "{}", err);
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(transparent)]
    struct TupleWrapper(#[serde(with = "tuples")] Vec<(String, u32)>);

    #[test]
    fn tuple_serialization() {
        let tuples = vec![("test".to_owned(), 7), ("other".to_owned(), 42)];
        let wrapper = TupleWrapper(tuples.clone());
        let serialized = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(serialized, r#"{"test":7,"other":42}"#);

        let restored: TupleWrapper = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.0, tuples);
    }
}
