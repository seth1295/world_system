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

/// Canonicalizes general I-JSON according to RFC 8785, including binary64 values.
///
/// [`canonicalize_json`] remains the stricter VEYRA hashed-document path, which rejects
/// non-integer JSON numbers and integers outside the exact interoperable range.
pub fn canonicalize_rfc8785_json(input: &[u8]) -> Result<Vec<u8>, JcsError> {
    let value = parse_unique_json(input)?;
    canonicalize_rfc8785_value(&value)
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

/// Canonicalizes a parsed JSON value using RFC 8785 number serialization.
pub fn canonicalize_rfc8785_value(value: &Value) -> Result<Vec<u8>, JcsError> {
    let mut output = Vec::new();
    write_rfc8785_value(value, &mut output)?;
    Ok(output)
}

/// Serializes one finite binary64 value according to RFC 8785 Appendix B / ECMAScript.
pub fn serialize_rfc8785_number(value: f64) -> Result<String, JcsError> {
    if !value.is_finite() {
        return Err(JcsError::InvalidNumber);
    }
    if value == 0.0 {
        return Ok("0".to_owned());
    }

    let raw = serde_json::Number::from_f64(value).ok_or(JcsError::InvalidNumber)?.to_string();
    let (mantissa, exponent) = if let Some((mantissa, exponent)) = raw.split_once('e') {
        (mantissa, exponent.parse::<i32>().map_err(|_| JcsError::InvalidNumber)?)
    } else if let Some((mantissa, exponent)) = raw.split_once('E') {
        (mantissa, exponent.parse::<i32>().map_err(|_| JcsError::InvalidNumber)?)
    } else {
        (raw.as_str(), 0_i32)
    };
    let negative = mantissa.starts_with('-');
    let unsigned = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let decimal_position = unsigned.find('.').unwrap_or(unsigned.len());
    let mut digits: Vec<u8> = unsigned.bytes().filter(u8::is_ascii_digit).collect();
    let leading_zeros = digits.iter().take_while(|digit| **digit == b'0').count();
    if leading_zeros == digits.len() {
        return Ok("0".to_owned());
    }
    digits.drain(..leading_zeros);
    let decimal_position = i32::try_from(decimal_position).map_err(|_| JcsError::InvalidNumber)?
        + exponent
        - i32::try_from(leading_zeros).map_err(|_| JcsError::InvalidNumber)?;
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }

    let scientific = value.abs() < 1.0e-6 || value.abs() >= 1.0e21;
    let mut result = String::with_capacity(32);
    if negative {
        result.push('-');
    }
    if scientific {
        result.push(char::from(digits[0]));
        if digits.len() > 1 {
            result.push('.');
            result
                .push_str(core::str::from_utf8(&digits[1..]).map_err(|_| JcsError::InvalidNumber)?);
        }
        result.push('e');
        let exponent = decimal_position - 1;
        if exponent >= 0 {
            result.push('+');
        }
        result.push_str(&exponent.to_string());
    } else if decimal_position <= 0 {
        result.push_str("0.");
        for _ in 0..decimal_position.unsigned_abs() {
            result.push('0');
        }
        result.push_str(core::str::from_utf8(&digits).map_err(|_| JcsError::InvalidNumber)?);
    } else if usize::try_from(decimal_position).map_err(|_| JcsError::InvalidNumber)?
        >= digits.len()
    {
        result.push_str(core::str::from_utf8(&digits).map_err(|_| JcsError::InvalidNumber)?);
        for _ in
            digits.len()..usize::try_from(decimal_position).map_err(|_| JcsError::InvalidNumber)?
        {
            result.push('0');
        }
    } else {
        let split = usize::try_from(decimal_position).map_err(|_| JcsError::InvalidNumber)?;
        result
            .push_str(core::str::from_utf8(&digits[..split]).map_err(|_| JcsError::InvalidNumber)?);
        result.push('.');
        result
            .push_str(core::str::from_utf8(&digits[split..]).map_err(|_| JcsError::InvalidNumber)?);
    }
    Ok(result)
}

fn write_rfc8785_value(value: &Value, output: &mut Vec<u8>) -> Result<(), JcsError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Number(number) => {
            let value = number_to_binary64(number)?;
            output.extend_from_slice(serialize_rfc8785_number(value)?.as_bytes());
        }
        Value::String(text) => output.extend_from_slice(
            serde_json::to_string(text).map_err(|_| JcsError::InvalidString)?.as_bytes(),
        ),
        Value::Array(items) => {
            output.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_rfc8785_value(item, output)?;
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
                write_rfc8785_value(&properties[key], output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

fn number_to_binary64(number: &serde_json::Number) -> Result<f64, JcsError> {
    if let Some(value) = number.as_i64() {
        let binary64 = value as f64;
        if binary64 < -9_223_372_036_854_775_808.0
            || binary64 >= 9_223_372_036_854_775_808.0
            || binary64 as i64 != value
        {
            return Err(JcsError::InvalidNumber);
        }
        return Ok(binary64);
    }
    if let Some(value) = number.as_u64() {
        let binary64 = value as f64;
        if binary64 >= 18_446_744_073_709_551_616.0 || binary64 as u64 != value {
            return Err(JcsError::InvalidNumber);
        }
        return Ok(binary64);
    }
    number.as_f64().ok_or(JcsError::InvalidNumber)
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
    /// A JSON number is outside the finite binary64 range.
    InvalidNumber,
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
            Self::InvalidNumber => "RFC 8785 requires a finite binary64 number",
        })
    }
}

impl std::error::Error for JcsError {}

#[cfg(test)]
mod tests {
    use super::{
        JcsError, canonicalize_json, canonicalize_rfc8785_json, parse_unique_json,
        serialize_rfc8785_number,
    };

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
    fn general_rfc8785_serializes_binary64_while_hashed_json_stays_integer_only() {
        assert_eq!(canonicalize_json(br#"{"n":1.5}"#), Err(JcsError::ForbiddenNumber));
        assert_eq!(canonicalize_rfc8785_json(br#"{"n":1.5}"#).unwrap(), br#"{"n":1.5}"#);
        assert_eq!(canonicalize_rfc8785_json(br#"{"n":1e-6}"#).unwrap(), br#"{"n":0.000001}"#);
        assert_eq!(canonicalize_rfc8785_json(br#"{"n":1e+21}"#).unwrap(), br#"{"n":1e+21}"#);
        assert_eq!(canonicalize_rfc8785_json(b"9007199254740992").unwrap(), b"9007199254740992");
        assert_eq!(canonicalize_rfc8785_json(b"9007199254740993"), Err(JcsError::InvalidNumber));
        assert_eq!(serialize_rfc8785_number(-0.0).unwrap(), "0");
        assert_eq!(serialize_rfc8785_number(f64::NAN), Err(JcsError::InvalidNumber));
        assert_eq!(serialize_rfc8785_number(f64::INFINITY), Err(JcsError::InvalidNumber));
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
