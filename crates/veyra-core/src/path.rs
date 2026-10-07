//! Host-independent validation for literal VEYRA artifact-relative paths.

use core::fmt;

/// Validates a canonical slash-separated path inside one artifact.
pub fn validate_artifact_path(path: &str) -> Result<(), ArtifactPathError> {
    let bytes = path.as_bytes();
    let has_drive_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || has_drive_prefix
        || path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.contains(':')
                || part.ends_with([' ', '.'])
                || part.chars().any(is_windows_forbidden_character)
                || is_windows_device_name(part)
        })
    {
        return Err(ArtifactPathError::InvalidPath);
    }
    Ok(())
}

fn is_windows_forbidden_character(character: char) -> bool {
    matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
        || (character.is_ascii() && (character as u32) <= 0x1f)
}

fn is_windows_device_name(component: &str) -> bool {
    let basename = component.split('.').next().unwrap_or_default();
    // Windows reserves these basenames even when a component has an extension.
    if ["CON", "PRN", "AUX", "NUL"].iter().any(|name| basename.eq_ignore_ascii_case(name)) {
        return true;
    }

    let bytes = basename.as_bytes();
    if bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
    {
        return true;
    }

    ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"]
        .iter()
        .any(|name| basename.eq_ignore_ascii_case(name))
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
        for valid in [
            "console",
            "null",
            "COM0",
            "COM10",
            "LPT0",
            "LPT10",
            "xCON",
            "CONx",
            "company.txt",
            ".hidden",
            "café/世界.json",
            "delete\u{7f}character",
        ] {
            assert_eq!(validate_artifact_path(valid), Ok(()), "rejected {valid:?}");
        }
        for invalid in [
            "",
            "/x",
            "/absolute",
            "a/./b",
            "a//b",
            "../x",
            "a/../x",
            "a\\b",
            "C:/x",
            "C:relative",
            "a/C:/x",
            "a:b",
            "a/",
            "a\0b",
            "a\x01b",
            "a\x1fb",
            "bad<name",
            "bad>name",
            "bad\"name",
            "bad|name",
            "bad?name",
            "bad*name",
            "trailing ",
            "trailing.",
            "dir./file",
            "dir /file",
            "CON",
            "con",
            "Con.txt",
            "NUL",
            "nul.json",
            "PRN",
            "AUX",
            "COM1",
            "com9.bin",
            "LPT1",
            "lpt9.data",
            "registry/NUL",
            "registry/COM1.txt",
            "COM¹",
            "COM².ext",
            "LPT³",
            "LPT³.foo",
        ] {
            assert_eq!(
                validate_artifact_path(invalid),
                Err(ArtifactPathError::InvalidPath),
                "accepted {invalid:?}"
            );
        }

        for prefix in ["COM", "LPT"] {
            for digit in '1'..='9' {
                for basename in [format!("{prefix}{digit}"), format!("{prefix}{digit}.data")] {
                    assert_eq!(
                        validate_artifact_path(&basename),
                        Err(ArtifactPathError::InvalidPath),
                        "accepted reserved basename {basename:?}"
                    );
                }
            }
            for digit in ['\u{00b9}', '\u{00b2}', '\u{00b3}'] {
                let basename = format!("{prefix}{digit}");
                let with_extension = format!("{basename}.data");
                assert_eq!(validate_artifact_path(&basename), Err(ArtifactPathError::InvalidPath));
                assert_eq!(
                    validate_artifact_path(&with_extension),
                    Err(ArtifactPathError::InvalidPath)
                );
            }
        }
    }

    #[test]
    fn every_ascii_control_character_is_rejected() {
        for control in 1_u8..=31 {
            let component = format!("file{}name", char::from(control));
            assert_eq!(
                validate_artifact_path(&component),
                Err(ArtifactPathError::InvalidPath),
                "accepted ASCII control U+{control:04X}"
            );
        }
    }
}
