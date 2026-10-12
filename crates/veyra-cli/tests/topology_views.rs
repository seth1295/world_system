use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use veyra_core::canon::{hash, jcs};
use veyra_core::spatial::{DirCube, Topology};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "veyra-cli-topology-views-{}-{}",
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

#[test]
fn cli_lists_and_serves_topology_views_without_fields() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/worlds/cb9-minimal-void");
    let temp = TempDir::new();
    let body_dir = temp.0.join("body");
    copy_tree(&fixture, &body_dir);

    let body_path = body_dir.join("body.json");
    let mut body: Value = serde_json::from_slice(&fs::read(&body_path).unwrap()).unwrap();
    body["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    body["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":2
    }]);
    let canonical = jcs::canonicalize_json(&serde_json::to_vec(&body).unwrap()).unwrap();
    let baseline = hash::hash(&canonical);
    fs::write(&body_path, serde_json::to_vec_pretty(&body).unwrap()).unwrap();
    fs::write(body_dir.join("body.id"), format!("bas:{baseline}\n")).unwrap();

    let verify = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "verify"])
        .arg(&body_dir)
        .output()
        .expect("verify fieldless topology body");
    assert!(verify.status.success(), "{}", String::from_utf8_lossy(&verify.stderr));

    let views = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "views"])
        .arg(&body_dir)
        .output()
        .expect("list topology views");
    assert!(views.status.success(), "{}", String::from_utf8_lossy(&views.stderr));
    let views_json: Value = serde_json::from_slice(&views.stdout).expect("views JSON");
    assert!(
        views_json
            .as_array()
            .unwrap()
            .iter()
            .any(|view| { view["id"] == "topology.cube_face" && view["domain"] == "surface" })
    );

    let cell = DirCube::key(3, 0, 0, 1).unwrap();
    let tile = DirCube.tile_key(cell, 0).unwrap();
    let tile_arg = format!("{}:0x{:016x}", tile.level, tile.address.0);
    let output = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "tile"])
        .arg(&body_dir)
        .args(["--domain", "surface", "--key", &tile_arg, "--view", "derived:topology.cube_face"])
        .output()
        .expect("request fieldless topology tile");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let tile_json: Value = serde_json::from_slice(&output.stdout).expect("tile JSON");
    assert_eq!(tile_json["values"], json!([3.0]));
}
