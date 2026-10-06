//! Canonical universe time representation.

use core::fmt;
use core::str::FromStr;

/// Signed nanoseconds from the universe epoch.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UTime(i128);

impl UTime {
    /// Constructs a timestamp from signed nanoseconds.
    pub const fn from_nanos(nanos: i128) -> Self {
        Self(nanos)
    }

    /// Returns signed nanoseconds from the universe epoch.
    pub const fn as_nanos(self) -> i128 {
        self.0
    }

    /// Returns the canonical decimal-string form.
    pub fn decimal(self) -> String {
        self.0.to_string()
    }
}

impl fmt::Display for UTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for UTime {
    type Err = TimeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value = text.parse::<i128>().map_err(|_| TimeError::InvalidDecimal)?;
        if value.to_string() != text {
            return Err(TimeError::NonCanonicalDecimal);
        }
        Ok(Self(value))
    }
}

/// Canonical decimal-string quantities without floating-point conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalString {
    text: String,
    negative: bool,
    zero: bool,
    coefficient: String,
    exponent10: i64,
}

impl DecimalString {
    /// Validates and retains a base-ten quantity without binary-float conversion.
    pub fn parse(text: &str) -> Result<Self, TimeError> {
        let (mantissa, exponent) = match text.split_once('e') {
            Some((mantissa, exponent)) if !exponent.is_empty() && !exponent.contains('e') => {
                let parsed = exponent.parse::<i32>().map_err(|_| TimeError::InvalidDecimal)?;
                if parsed.to_string() != exponent {
                    return Err(TimeError::NonCanonicalDecimal);
                }
                (mantissa, parsed)
            }
            Some(_) => return Err(TimeError::InvalidDecimal),
            None => (text, 0),
        };
        let (negative, unsigned) =
            mantissa.strip_prefix('-').map_or((false, mantissa), |rest| (true, rest));
        let mut pieces = unsigned.split('.');
        let integer = pieces.next().unwrap_or_default();
        let fraction = pieces.next();
        if pieces.next().is_some()
            || integer.is_empty()
            || (integer.len() > 1 && integer.starts_with('0'))
            || !integer.bytes().all(|byte| byte.is_ascii_digit())
            || fraction
                .is_some_and(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(TimeError::InvalidDecimal);
        }
        let fraction = fraction.unwrap_or_default();
        // The exponent token is canonical i32, but subtracting fractional precision can
        // move the effective base-ten exponent just outside i32. Keep that derived value
        // in i64 so every canonical i32 exponent remains representable.
        let fraction_len = i64::try_from(fraction.len()).map_err(|_| TimeError::InvalidDecimal)?;
        let exponent10 =
            i64::from(exponent).checked_sub(fraction_len).ok_or(TimeError::InvalidDecimal)?;
        let digits = format!("{integer}{fraction}");
        let significant = digits.trim_start_matches('0');
        let zero = significant.is_empty();
        if negative && zero {
            return Err(TimeError::NonCanonicalDecimal);
        }
        Ok(Self {
            text: text.to_owned(),
            negative,
            zero,
            coefficient: if zero { "0".to_owned() } else { significant.to_owned() },
            exponent10: if zero { 0 } else { exponent10 },
        })
    }

    /// Returns the validated decimal string.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns whether the represented quantity is negative.
    pub const fn is_negative(&self) -> bool {
        self.negative
    }

    /// Returns whether every significant mantissa digit is zero.
    pub const fn is_zero(&self) -> bool {
        self.zero
    }

    /// Returns the exact nonnegative significand digits with sign stored separately.
    pub fn coefficient_digits(&self) -> &str {
        &self.coefficient
    }

    /// Returns the power of ten multiplied by the coefficient digits.
    pub const fn exponent10(&self) -> i64 {
        self.exponent10
    }

    /// Converts to IEEE-754 binary64 using Rust's correctly rounded decimal parser.
    pub fn to_f64(&self) -> Result<f64, TimeError> {
        let value = self.text.parse::<f64>().map_err(|_| TimeError::OutOfRange)?;
        if !value.is_finite() {
            return Err(TimeError::OutOfRange);
        }
        Ok(value)
    }
}

/// Timestamp or decimal-string parsing failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeError {
    /// The text is not in the accepted decimal grammar or is out of range.
    InvalidDecimal,
    /// The value parses but is not in canonical text form.
    NonCanonicalDecimal,
    /// The decimal value is outside the finite binary64 range.
    OutOfRange,
}

impl fmt::Display for TimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDecimal => "invalid decimal string",
            Self::NonCanonicalDecimal => "decimal string is not canonical",
            Self::OutOfRange => "decimal string is outside the finite binary64 range",
        })
    }
}

impl std::error::Error for TimeError {}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::{DecimalString, TimeError, UTime};

    #[test]
    fn utime_is_i128_and_text_is_canonical() {
        let value = UTime::from_str("-170141183460469231731687303715884105728").unwrap();
        assert_eq!(value.as_nanos(), i128::MIN);
        assert!(matches!(UTime::from_str("-0"), Err(TimeError::NonCanonicalDecimal)));
        assert!(matches!(UTime::from_str("01"), Err(TimeError::NonCanonicalDecimal)));
    }

    #[test]
    fn decimal_string_does_not_round_through_float() {
        let value = DecimalString::parse("0.0000000000000000000000000000000001").unwrap();
        assert_eq!(value.as_str(), "0.0000000000000000000000000000000001");
        let scientific = DecimalString::parse("6.67430e-11").unwrap();
        assert_eq!(scientific.as_str(), "6.67430e-11");
        assert_eq!(scientific.coefficient_digits(), "667430");
        assert_eq!(scientific.exponent10(), -16);
        assert_eq!(
            scientific.to_f64().unwrap().to_bits(),
            "6.67430e-11".parse::<f64>().unwrap().to_bits()
        );
        assert_eq!(
            DecimalString::parse("9007199254740993").unwrap().to_f64().unwrap().to_bits(),
            9_007_199_254_740_992_f64.to_bits()
        );
        assert!(matches!(
            DecimalString::parse("1e9999").unwrap().to_f64(),
            Err(TimeError::OutOfRange)
        ));
        assert!(DecimalString::parse("01.2").is_err());
        assert!(DecimalString::parse("1E3").is_err());
        assert!(DecimalString::parse("0e3").unwrap().is_zero());
        assert!(DecimalString::parse("-0.0").is_err());
    }

    #[test]
    fn decimal_exponent_grammar_matches_canonical_i32_boundaries() {
        for text in ["1e2147483647", "1e-2147483648", "1.0e2147483647", "1.0e-2147483648"] {
            assert!(DecimalString::parse(text).is_ok(), "rejected boundary {text}");
        }
        assert_eq!(DecimalString::parse("1.0e-2147483648").unwrap().exponent10(), -2_147_483_649);

        for text in ["1e2147483648", "1e-2147483649", "1e-0", "1e00", "1e+1", "1E3", "1e"] {
            assert!(DecimalString::parse(text).is_err(), "accepted noncanonical exponent {text}");
        }
    }
}
