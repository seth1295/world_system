//! Pure hash-chained JSON Lines ledger operations.

use core::fmt;

use serde_json::{Map, Value};

use crate::canon::{hash, jcs};
use crate::ids::Hash32;
use crate::time::UTime;

/// The all-zero predecessor used by the first ledger entry.
pub const GENESIS_PREV: Hash32 = Hash32([0; 32]);

/// One verified ledger head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerHead {
    /// Number of entries verified.
    pub count: u64,
    /// Last entry hash, or the genesis predecessor for an empty ledger.
    pub hash: Hash32,
}

/// Appends one canonical hash-chained JSONL ledger entry.
pub fn append(
    previous: &[u8],
    entry_type: &str,
    payload: Value,
    time: UTime,
) -> Result<(Vec<u8>, Hash32), LedgerError> {
    let head = verify(previous)?;
    let mut object = Map::new();
    object.insert("seq".to_owned(), Value::from(head.count));
    object.insert("prev".to_owned(), Value::String(head.hash.to_string()));
    object.insert("type".to_owned(), Value::String(entry_type.to_owned()));
    object.insert("payload".to_owned(), payload);
    object.insert("t".to_owned(), Value::String(time.decimal()));
    let digest = hash::hash(
        &jcs::canonicalize_value(&Value::Object(object.clone()))
            .map_err(|_| LedgerError::InvalidEntry)?,
    );
    object.insert("hash".to_owned(), Value::String(digest.to_string()));
    let line =
        jcs::canonicalize_value(&Value::Object(object)).map_err(|_| LedgerError::InvalidEntry)?;
    let mut output = previous.to_vec();
    if !output.is_empty() && output.last() != Some(&b'\n') {
        output.push(b'\n');
    }
    output.extend_from_slice(&line);
    output.push(b'\n');
    Ok((output, digest))
}

/// Verifies every sequence number, predecessor, and entry hash in JSONL bytes.
pub fn verify(bytes: &[u8]) -> Result<LedgerHead, LedgerError> {
    let mut head = LedgerHead { count: 0, hash: GENESIS_PREV };
    if bytes.is_empty() {
        return Ok(head);
    }
    let lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let count = lines.len().saturating_sub(usize::from(bytes.last() == Some(&b'\n')));
    for line in lines.into_iter().take(count) {
        if line.is_empty() {
            return Err(LedgerError::InvalidEntry);
        }
        jcs::canonicalize_json(line).map_err(|_| LedgerError::InvalidEntry)?;
        let mut value: Value =
            serde_json::from_slice(line).map_err(|_| LedgerError::InvalidEntry)?;
        let object = value.as_object_mut().ok_or(LedgerError::InvalidEntry)?;
        let entry_hash = object
            .remove("hash")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(LedgerError::InvalidEntry)?;
        let sequence =
            object.get("seq").and_then(Value::as_u64).ok_or(LedgerError::InvalidEntry)?;
        let previous =
            object.get("prev").and_then(Value::as_str).ok_or(LedgerError::InvalidEntry)?;
        if sequence != head.count || previous != head.hash.to_string() {
            return Err(LedgerError::BrokenChain);
        }
        let calculated =
            hash::hash(&jcs::canonicalize_value(&value).map_err(|_| LedgerError::InvalidEntry)?);
        if calculated.to_string() != entry_hash {
            return Err(LedgerError::TamperedEntry);
        }
        head.hash = calculated;
        head.count = head.count.checked_add(1).ok_or(LedgerError::InvalidEntry)?;
    }
    Ok(head)
}

/// Ledger verification failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerError {
    /// An entry is not a valid hashable JSON object.
    InvalidEntry,
    /// Sequence or predecessor does not continue the chain.
    BrokenChain,
    /// An entry's content does not match its hash.
    TamperedEntry,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEntry => "invalid ledger entry",
            Self::BrokenChain => "ledger predecessor or sequence is broken",
            Self::TamperedEntry => "ledger entry hash does not match its content",
        })
    }
}

impl std::error::Error for LedgerError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{LedgerError, append, verify};
    use crate::time::UTime;

    #[test]
    fn appends_and_verifies_hash_chain() {
        let first =
            append(&[], "created", json!({"value": "one"}), UTime::from_nanos(0)).unwrap().0;
        let second =
            append(&first, "updated", json!({"value": "two"}), UTime::from_nanos(1)).unwrap().0;
        assert_eq!(verify(&second).unwrap().count, 2);
        let mut tampered = second;
        let position = tampered.iter().position(|byte| *byte == b't').unwrap();
        tampered[position] = b'x';
        assert!(matches!(
            verify(&tampered),
            Err(LedgerError::TamperedEntry) | Err(LedgerError::BrokenChain)
        ));
    }

    #[test]
    fn refuses_fractional_values_in_hashed_entries() {
        let result = append(&[], "created", json!({"value": 1.25}), UTime::from_nanos(0));
        assert!(result.is_err());
    }

    #[test]
    fn append_repairs_a_missing_terminal_newline_and_verify_rejects_duplicate_names() {
        let first = append(&[], "created", json!({"value":"one"}), UTime::from_nanos(0)).unwrap().0;
        let unterminated = first[..first.len() - 1].to_vec();
        let second = append(&unterminated, "updated", json!({"value":"two"}), UTime::from_nanos(1))
            .unwrap()
            .0;
        assert_eq!(verify(&second).unwrap().count, 2);

        let duplicate =
            String::from_utf8(first).unwrap().replacen("\"seq\":0,", "\"seq\":0,\"seq\":0,", 1);
        assert_eq!(verify(duplicate.as_bytes()), Err(LedgerError::InvalidEntry));
    }
}
