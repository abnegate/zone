use std::fmt;

use serde::Deserializer;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::scim::folded::Folded;

pub(crate) struct FoldedVisitor;

impl<'de> Visitor<'de> for FoldedVisitor {
    type Value = Folded;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Folded, E> {
        Ok(Folded(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Folded, E> {
        Ok(Folded(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Folded, E> {
        Ok(Folded(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Folded, E> {
        Ok(Folded(
            Number::from_f64(value).map_or(Value::Null, Value::Number),
        ))
    }

    fn visit_str<E>(self, value: &str) -> Result<Folded, E> {
        Ok(Folded(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Folded, E> {
        Ok(Folded(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Folded, E> {
        Ok(Folded(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Folded, E> {
        Ok(Folded(Value::Null))
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Folded, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Folded, A::Error> {
        let mut values = Vec::new();
        while let Some(Folded(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(Folded(Value::Array(values)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Folded, A::Error> {
        let mut object = Map::new();
        while let Some((key, Folded(value))) = entries.next_entry::<String, Folded>()? {
            object.retain(|existing, _| !existing.eq_ignore_ascii_case(&key));
            object.insert(key, value);
        }
        Ok(Folded(Value::Object(object)))
    }
}
