use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "veyra-cli-canonical-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn canonical_fmt_stdout_is_exact_jcs_and_hashes_as_canonical_json() {
    let directory = TempDir::new();
    let input_path = directory.0.join("input.json");
    let redirected_path = directory.0.join("canonical.json");
    let input = br#"{"z":2,"a":"line\nvalue"}"#;
    fs::write(&input_path, input).expect("write input");

    let formatted = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["fmt", "--canonical"])
        .arg(&input_path)
        .output()
        .expect("run canonical formatter");
    assert!(
        formatted.status.success(),
        "fmt failed: {}",
        String::from_utf8_lossy(&formatted.stderr)
    );
    let expected = veyra_core::canon::jcs::canonicalize_json(input).unwrap();
    assert_eq!(formatted.stdout, expected);
    assert!(!formatted.stdout.ends_with(b"\n"));

    fs::write(&redirected_path, &formatted.stdout).expect("redirect output bytes");
    let raw_hash = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .arg("hash")
        .arg(&redirected_path)
        .output()
        .expect("hash canonical bytes");
    let canonical_hash = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .arg("hash-json")
        .arg(&input_path)
        .output()
        .expect("hash canonical JSON");
    assert!(raw_hash.status.success());
    assert!(canonical_hash.status.success());
    assert_eq!(raw_hash.stdout, canonical_hash.stdout);
}
