use std::path::Path;
use std::process::Command;

use veyra_core::io::LoaderError;
use veyra_writer::{WriterError, open_directory};

#[test]
fn cli_and_library_report_the_same_tampered_blob_code_and_hash() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let fixture = root.join("conformance/worlds/cb6-compat/tampered-blob");
    let library_error = open_directory(&fixture).unwrap_err();
    let (code, hash) = match library_error {
        WriterError::Loader(LoaderError::HashMismatch(hash)) => ("hash_mismatch", hash.to_string()),
        error => panic!("unexpected library error: {error:?}"),
    };

    let output = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "verify"])
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&format!("error[{code}]")), "{stderr}");
    assert!(stderr.contains(&hash), "{stderr}");
}
