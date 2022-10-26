//! Helpers to (de)serialize some parts of `TracingEvent`s.

#[cfg(feature = "receiver")]
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

#[cfg(all(test, feature = "receiver"))]
mod tests {
    use serde::{Deserialize, Serialize};
    use tracing_core::span::Id;

    use super::span_id;

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
}
