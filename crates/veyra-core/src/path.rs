//! Host-independent validation for literal VEYRA artifact-relative paths.

use core::fmt;

use unicode_normalization::UnicodeNormalization;

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

/// Returns a host-independent Unicode collision key without changing the declared path.
///
/// The collision policy applies Unicode uppercase mapping followed by NFC normalization to the
/// complete slash-separated path. Original path text remains the artifact reference; this key is
/// only for checking case and normalization aliases. Two paths collide when their keys are equal,
/// or when one key is a complete component prefix of the other (a file-versus-directory clash).
pub fn artifact_path_collision_key(path: &str) -> String {
    path.chars().flat_map(char::to_uppercase).collect::<String>().nfc().collect()
}

/// Reports equality or a file-versus-directory prefix collision between collision keys.
pub fn artifact_path_keys_collide(left: &str, right: &str) -> bool {
    left == right
        || left.strip_prefix(right).is_some_and(|suffix| suffix.starts_with('/'))
        || right.strip_prefix(left).is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_windows_forbidden_character(character: char) -> bool {
    matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
        || (character.is_ascii() && (character as u32) <= 0x1f)
}

fn is_windows_device_name(component: &str) -> bool {
    let full_key = artifact_path_collision_key(component);
    if ["CONIN$", "CONOUT$"].contains(&full_key.as_str()) {
        return true;
    }
    let basename = component.split('.').next().unwrap_or_default();
    let basename_key = artifact_path_collision_key(basename);
    // Windows reserves these basenames even when a component has an extension.
    if ["CON", "PRN", "AUX", "NUL"].contains(&basename_key.as_str()) {
        return true;
    }

    let bytes = basename_key.as_bytes();
    if bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
    {
        return true;
    }

    ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"].iter().any(|name| basename_key == *name)
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
    use super::{
        ArtifactPathError, artifact_path_collision_key, artifact_path_keys_collide,
        validate_artifact_path,
    };

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
            "CONIN$",
            "conin$",
            "ConIn$",
            "con\u{0131}n$",
            "CONOUT$",
            "conout$",
            "folder/conOut$",
            "folder/CONOUT$",
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
        for valid in ["CONIN$.txt", "CONOUT$.log", "CONIN$device", "conout$.data"] {
            assert_eq!(validate_artifact_path(valid), Ok(()), "rejected {valid:?}");
        }
    }

    #[test]
    fn collision_keys_cover_unicode_case_normalization_and_directory_aliases() {
        for (left, right) in [
            ("sections/State.json", "sections/state.JSON"),
            ("Résumé/Field.json", "résumé/field.JSON"),
            ("Σ/data.json", "ς/DATA.JSON"),
            ("café/data.json", "cafe\u{301}/DATA.JSON"),
            ("straße/data.json", "STRASSE/DATA.JSON"),
            ("Directory", "directory/child.json"),
            ("Directory/child.json", "directory"),
        ] {
            let left_key = artifact_path_collision_key(left);
            let right_key = artifact_path_collision_key(right);
            assert!(
                artifact_path_keys_collide(&left_key, &right_key),
                "{left:?} and {right:?} did not collide"
            );
            assert!(
                artifact_path_keys_collide(&right_key, &left_key),
                "collision rule was not symmetric for {left:?} and {right:?}"
            );
        }

        let left = artifact_path_collision_key("Data/one.json");
        let right = artifact_path_collision_key("data/two.json");
        assert!(!artifact_path_keys_collide(&left, &right));
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
