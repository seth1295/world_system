//! Host-independent validation for literal VEYRA artifact-relative paths.

use core::fmt;

/// Validates a canonical slash-separated path inside one artifact.
pub fn validate_artifact_path(path: &str) -> Result<(), ArtifactPathError> {
    let bytes = path.as_bytes();
    let has_drive_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || has_drive_prefix
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
    {
        return Err(ArtifactPathError::InvalidPath);
    }
    Ok(())
}

/// A literal artifact path violates the VEYRA slash-separated relative-path grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactPathError {
    /// Paths must contain nonempty normal slash-separated components.
    InvalidPath,
}

impl fmt::Display for ArtifactPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("artifact path must be a canonical relative path")
    }
}

impl std::error::Error for ArtifactPathError {}

#[cfg(test)]
mod tests {
    use super::{ArtifactPathError, validate_artifact_path};

    #[test]
    fn artifact_paths_use_the_same_literal_grammar_on_every_host() {
        assert!(validate_artifact_path("registry/fields.json").is_ok());
        for invalid in [
            "",
            "/x",
            "a/./b",
            "a//b",
            "../x",
            "a/../x",
            "a\\b",
            "C:/x",
            "C:relative",
            "a/C:/x",
            "a\0b",
        ] {
            assert_eq!(
                validate_artifact_path(invalid),
                Err(ArtifactPathError::InvalidPath),
                "accepted {invalid:?}"
            );
        }
    }
}
