//! RFC 8785-style canonical JSON for VEYRA hashed documents.

use core::fmt;
use core::fmt::Formatter;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use serde_json::Value;

/// Canonicalizes JSON after rejecting floating-point values and unsafe integers.
pub fn canonicalize_json(input: &[u8]) -> Result<Vec<u8>, JcsError> {
    let value = parse_unique_json(input)?;
    canonicalize_value(&value)
}

/// Parses JSON while rejecting duplicate object names without applying JCS number restrictions.
pub fn parse_unique_json(input: &[u8]) -> Result<Value, JcsError> {
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let value = UniqueValue::deserialize(&mut deserializer).map_err(|_| JcsError::InvalidJson)?.0;
    deserializer.end().map_err(|_| JcsError::InvalidJson)?;
    Ok(value)
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value with unique object names")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_unit()
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let number =
            serde_json::Number::from_f64(value).ok_or_else(|| E::custom("invalid float"))?;
        Ok(UniqueValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate JSON object name"));
            }
            values.insert(key, value.0);
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

/// Canonicalizes a parsed value using UTF-16 property ordering.
pub fn canonicalize_value(value: &Value) -> Result<Vec<u8>, JcsError> {
    let mut output = Vec::new();
    write_value(value, &mut output)?;
    Ok(output)
}

fn write_value(value: &Value, output: &mut Vec<u8>) -> Result<(), JcsError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                if signed.unsigned_abs() > 9_007_199_254_740_991 {
                    return Err(JcsError::UnsafeInteger);
                }
                output.extend_from_slice(signed.to_string().as_bytes());
            } else if let Some(unsigned) = number.as_u64() {
                if unsigned > 9_007_199_254_740_991 {
                    return Err(JcsError::UnsafeInteger);
                }
                output.extend_from_slice(unsigned.to_string().as_bytes());
            } else {
                return Err(JcsError::ForbiddenNumber);
            }
        }
        Value::String(text) => {
            output.extend_from_slice(
                serde_json::to_string(text).map_err(|_| JcsError::InvalidString)?.as_bytes(),
            );
        }
        Value::Array(items) => {
            output.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_value(item, output)?;
            }
            output.push(b']');
        }
        Value::Object(properties) => {
            let mut keys: Vec<&String> = properties.keys().collect();
            keys.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
            output.push(b'{');
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                output.extend_from_slice(
                    serde_json::to_string(key).map_err(|_| JcsError::InvalidString)?.as_bytes(),
                );
                output.push(b':');
                write_value(&properties[key], output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// JCS parser, number, or string error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JcsError {
    /// Input is not valid JSON.
    InvalidJson,
    /// JSON contains a fractional or exponent-form number.
    ForbiddenNumber,
    /// An integer is outside the exact interoperable JSON range.
    UnsafeInteger,
    /// A string cannot be represented as valid JSON text.
    InvalidString,
}

impl fmt::Display for JcsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidJson => "invalid JSON",
            Self::ForbiddenNumber => "hashed JSON cannot contain a non-integer number",
            Self::UnsafeInteger => {
                "hashed JSON integer must be within the exact interoperable range"
            }
            Self::InvalidString => "JSON string cannot be encoded",
        })
    }
}

impl std::error::Error for JcsError {}

#[cfg(test)]
mod tests {
    use super::{JcsError, canonicalize_json, parse_unique_json};

    #[test]
    fn sorts_keys_canonically_and_preserves_unicode() {
        assert_eq!(canonicalize_json(br#"{"b":1,"a":"x"}"#).unwrap(), br#"{"a":"x","b":1}"#);
        let text = r#"{"":1,"😀":2}"#;
        assert_eq!(canonicalize_json(text.as_bytes()).unwrap(), r#"{"😀":2,"":1}"#.as_bytes());
    }

    #[test]
    fn rejects_fractional_exponent_and_unsafe_numbers() {
        assert_eq!(canonicalize_json(br#"{"n":1.5}"#), Err(JcsError::ForbiddenNumber));
        assert_eq!(canonicalize_json(br#"{"n":1e0}"#), Err(JcsError::ForbiddenNumber));
        assert_eq!(canonicalize_json(br#"{"n":9007199254740992}"#), Err(JcsError::UnsafeInteger));
    }

    #[test]
    fn rejects_duplicate_object_names_in_hashed_documents() {
        assert_eq!(canonicalize_json(br#"{"a":1,"a":2}"#), Err(JcsError::InvalidJson));
        assert_eq!(canonicalize_json(br#"{"a":1,"\u0061":2}"#), Err(JcsError::InvalidJson));
    }

    #[test]
    fn unique_json_parser_rejects_duplicate_names_without_restricting_pretty_numbers() {
        assert_eq!(parse_unique_json(br#"{"root":1,"root":2}"#), Err(JcsError::InvalidJson));
        assert_eq!(
            parse_unique_json(br#"{"outer":{"inner":1,"inner":2}}"#),
            Err(JcsError::InvalidJson)
        );
        assert!(parse_unique_json(br#"{"values":[1,1,1.5]}"#).is_ok());
    }
}
