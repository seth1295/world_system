use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;
use veyra_core::spatial::{EdgeAdjacency, FACE_ADJACENCY, FaceEdge};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[test]
fn embedded_adjacency_matches_table_in_architecture_schema() {
    let source = fs::read_to_string(repository_root().join("schema/face_adjacency.toml")).unwrap();
    let document: Value = toml::from_str(&source).unwrap();
    let entries = document["mapping"].as_array().unwrap();
    assert_eq!(entries.len(), 24);
    for entry in entries {
        let face = entry["face"].as_integer().unwrap() as usize;
        let source_edge = entry["edge"].as_str().unwrap();
        let edge_index = edge_index(source_edge);
        let actual = FACE_ADJACENCY[face][edge_index];
        let expected_edge = edge_name(entry["to_edge"].as_str().unwrap());
        let expected = EdgeAdjacency {
            face: entry["to_face"].as_integer().unwrap() as u8,
            edge: expected_edge,
            flip: entry["flip"].as_bool().unwrap(),
        };
        assert_eq!(actual, expected, "face {face} edge {source_edge}");
    }
}

#[test]
fn core_source_has_no_io_authority_or_terrestrial_vocabulary() {
    let core = repository_root().join("crates/veyra-core/src");
    let mut sources = Vec::new();
    collect_rust_files(&core, &mut sources);
    let forbidden_terms = [
        "elevation",
        "sea",
        "ocean",
        "terrain",
        "soil",
        "season",
        "hemisphere",
        "planet",
        "earth",
        "terrestrial",
        "continent",
        "continental",
    ];
    let forbidden_io = ["std::fs", "std::net", "std::os::", "reqwest::", "tokio::fs"];
    for path in sources {
        let content = fs::read_to_string(&path).unwrap().to_lowercase();
        for term in forbidden_terms {
            assert!(
                !contains_word(&content, term),
                "forbidden core term {term} in {}",
                path.display()
            );
        }
        for import in forbidden_io {
            assert!(!content.contains(import), "core IO dependency {import} in {}", path.display());
        }
        assert!(!content.contains("unsafe {"), "unsafe block in {}", path.display());
    }
}

#[test]
fn workspace_dependencies_preserve_the_declared_direction() {
    let root = repository_root();
    let expected_core: BTreeSet<&str> =
        ["blake3", "libm", "ruzstd", "serde", "serde_json"].into_iter().collect();
    let core_manifest: Value =
        toml::from_str(&fs::read_to_string(root.join("crates/veyra-core/Cargo.toml")).unwrap())
            .unwrap();
    let core_dependencies: BTreeSet<&str> =
        core_manifest["dependencies"].as_table().unwrap().keys().map(String::as_str).collect();
    assert_eq!(core_dependencies, expected_core);

    assert_dependencies(
        &root,
        "veyra-writer",
        &["veyra-core", "zstd", "serde", "serde_json", "blake3"],
    );
    assert_dependencies(&root, "veyra-universe", &["veyra-core", "veyra-writer"]);
    assert_dependencies(&root, "veyra-bodygen", &["veyra-writer"]);
    assert_dependencies(&root, "veyra-wasm", &["veyra-core"]);
    assert_dependencies(&root, "veyra-capi", &["veyra-core"]);
    assert_dependencies(&root, "veyra-conformance", &["veyra-core", "veyra-writer", "serde_json"]);
    assert_dependencies(
        &root,
        "veyra-cli",
        &[
            "veyra-core",
            "veyra-writer",
            "veyra-universe",
            "veyra-bodygen",
            "veyra-conformance",
            "blake3",
            "serde_json",
        ],
    );
}

fn assert_dependencies(root: &Path, crate_name: &str, expected: &[&str]) {
    let manifest: Value = toml::from_str(
        &fs::read_to_string(root.join(format!("crates/{crate_name}/Cargo.toml"))).unwrap(),
    )
    .unwrap();
    let actual: BTreeSet<&str> =
        manifest["dependencies"].as_table().unwrap().keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(actual, expected, "dependency direction for {crate_name}");
}

fn collect_rust_files(directory: &Path, output: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(index, _)| {
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + needle.len()..].chars().next();
        !before.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
            && !after.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

fn edge_index(edge: &str) -> usize {
    match edge {
        "U-" => 0,
        "U+" => 1,
        "V-" => 2,
        "V+" => 3,
        _ => panic!("unknown edge {edge}"),
    }
}

fn edge_name(edge: &str) -> FaceEdge {
    match edge {
        "U-" => FaceEdge::UMinus,
        "U+" => FaceEdge::UPlus,
        "V-" => FaceEdge::VMinus,
        "V+" => FaceEdge::VPlus,
        _ => panic!("unknown edge {edge}"),
    }
}
