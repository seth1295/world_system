use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use veyra_core::canon::{hash, jcs};
use veyra_core::spatial::{DirCube, Topology};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "veyra-cli-duplicate-fields-{}-{}",
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

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create copied directory");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("fixture directory entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("fixture entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

fn duplicate_name_body() -> (TempDir, String, String, String, String) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/worlds/cb1-seams");
    let temp = TempDir::new();
    let body_dir = temp.0.join("body");
    copy_tree(&fixture, &body_dir);

    let body_path = body_dir.join("body.json");
    let mut body: Value = serde_json::from_slice(&fs::read(&body_path).unwrap()).unwrap();
    let registry_path =
        body_dir.join(body["sections"]["registry"]["path"].as_str().expect("registry path"));
    let mut registry: Value = serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
    let fields = registry["fields"].as_array_mut().unwrap();
    let duplicate_name = fields[0]["name"].as_str().unwrap().to_owned();
    let first_id = fields[0]["id"].as_str().unwrap().to_owned();
    let second_id = fields[1]["id"].as_str().unwrap().to_owned();
    let unique_name = fields[2]["name"].as_str().unwrap().to_owned();
    fields[1]["name"] = json!(duplicate_name);
    fields[1]["domain"] = json!("other");

    let mut second_domain = body["domains"][0].clone();
    second_domain["id"] = json!("other");
    body["domains"].as_array_mut().unwrap().push(second_domain);

    let registry_json = serde_json::to_vec(&registry).unwrap();
    let canonical_registry = jcs::canonicalize_json(&registry_json).unwrap();
    body["sections"]["registry"]["hash"] = json!(hash::hash(&canonical_registry).to_string());
    fs::write(&registry_path, serde_json::to_vec_pretty(&registry).unwrap())
        .expect("write duplicate-name registry");

    let canonical_body = jcs::canonicalize_json(&serde_json::to_vec(&body).unwrap()).unwrap();
    let baseline_id = hash::hash(&canonical_body);
    fs::write(&body_path, serde_json::to_vec_pretty(&body).unwrap()).expect("write body root");
    fs::write(body_dir.join("body.id"), format!("bas:{baseline_id}\n"))
        .expect("write body identity");
    (temp, duplicate_name, first_id, second_id, unique_name)
}

fn run_cli(args: &[&str], body_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(args.iter().take(2))
        .arg(body_dir)
        .args(args.iter().skip(2))
        .output()
        .expect("run body command")
}

#[test]
fn cli_rejects_duplicate_names_and_keeps_id_and_unique_name_selection() {
    let (temp, duplicate_name, first_id, second_id, unique_name) = duplicate_name_body();
    let body_dir = temp.0.join("body");
    let verify = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "verify"])
        .arg(&body_dir)
        .output()
        .expect("verify duplicate-name fixture");
    assert!(
        verify.status.success(),
        "fixture setup failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );

    let cell = DirCube::key(0, 0, 0, 2).unwrap();
    let cell_arg = format!("surface:0x{:016x}", cell.0);
    let duplicate_sample = run_cli(
        &["body", "sample", "--field", &duplicate_name, "--cell", &cell_arg, "--level", "2"],
        &body_dir,
    );
    assert!(!duplicate_sample.status.success());
    let duplicate_sample_error = String::from_utf8_lossy(&duplicate_sample.stderr);
    assert!(duplicate_sample_error.contains("ambiguous"), "{duplicate_sample_error}");
    assert!(duplicate_sample_error.contains("FieldId"), "{duplicate_sample_error}");

    let tile = DirCube.tile_key(cell, 2).unwrap();
    let tile_arg = format!("{}:0x{:016x}", tile.level, tile.address.0);
    let duplicate_tile = run_cli(
        &["body", "tile", "--field", &duplicate_name, "--key", &tile_arg, "--time", "static"],
        &body_dir,
    );
    assert!(!duplicate_tile.status.success());
    let duplicate_tile_error = String::from_utf8_lossy(&duplicate_tile.stderr);
    assert!(duplicate_tile_error.contains("ambiguous"), "{duplicate_tile_error}");
    assert!(duplicate_tile_error.contains("FieldId"), "{duplicate_tile_error}");

    let by_id = run_cli(
        &["body", "sample", "--field", &first_id, "--cell", &cell_arg, "--level", "2"],
        &body_dir,
    );
    assert!(by_id.status.success(), "{}", String::from_utf8_lossy(&by_id.stderr));
    let by_second_id = run_cli(
        &["body", "tile", "--field", &second_id, "--key", &tile_arg, "--time", "static"],
        &body_dir,
    );
    assert!(by_second_id.status.success(), "{}", String::from_utf8_lossy(&by_second_id.stderr));

    let by_unique_name = run_cli(
        &[
            "body",
            "sample",
            "--field",
            &unique_name,
            "--cell",
            &cell_arg,
            "--level",
            "2",
            "--time",
            "slice:0",
        ],
        &body_dir,
    );
    assert!(by_unique_name.status.success(), "{}", String::from_utf8_lossy(&by_unique_name.stderr));
}
