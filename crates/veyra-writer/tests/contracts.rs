use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use veyra_core::body::FieldId;
use veyra_core::canon::blob::{BlobKind, CanonicalBlob, DType};
use veyra_core::canon::hash;
use veyra_core::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
use veyra_core::ids::{ObjectAddress, ObjectId, UniverseId};
use veyra_core::spatial::DirCube;
use veyra_core::time::UTime;
use veyra_writer::{ArtifactWriter, WriterError, open_directory, verify_directory};

fn temp_path(label: &str) -> PathBuf {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("veyra-{label}-{}-{unique}", std::process::id()))
}

fn base_body(writer: &mut ArtifactWriter, fixture: &str, registry: Value) -> Value {
    let registry_hash = writer
        .write_json_section("registry/fields.json", &serde_json::to_vec(&registry).unwrap())
        .unwrap();
    let descriptor_hash = writer
        .write_json_section(
            "dynamics/descriptor.json",
            br#"{"schema":"veyra.dynamics_descriptor/1"}"#,
        )
        .unwrap();
    let origin_hash = writer
        .write_json_section("dynamics/origin.json", br#"{"schema":"veyra.dynamics_origin/1"}"#)
        .unwrap();
    let object_id = ObjectId::derive(
        UniverseId::fixture_sentinel(),
        &ObjectAddress::Fixture { name: fixture.to_owned() },
    )
    .unwrap();
    json!({
        "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
        "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1"],
        "identity":{"object_id":object_id.to_string(),"origin":{"kind":"fixture","name":fixture}},
        "classification":{},"physical":{"gm_m3_s2":"1"},"figure":{"kind":"sphere","radius_m":"1"},
        "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed","rotation":{"kind":"uniform","period_s":"86400","epoch":"0","orientation_q_at_epoch":["1","0","0","0"],"relative_to":"universe_inertial"}}},"reference_surfaces":[],
        "dynamics":{
            "descriptor":{"path":"dynamics/descriptor.json","hash":descriptor_hash.to_string()},
            "origin_keyframe":{"path":"dynamics/origin.json","hash":origin_hash.to_string()}
        },
        "capabilities":[],"domains":[],"codec":"zstd+shuffle2",
        "sections":{"registry":{"path":"registry/fields.json","hash":registry_hash.to_string()}},
        "indexes":{}
    })
}

fn append_extension() -> (Vec<u8>, veyra_core::ids::Hash32) {
    veyra_core::canon::ledger::append(
        &[],
        "sealed",
        json!({"extension":"ext:fixture"}),
        UTime::from_nanos(0),
    )
    .unwrap()
}

fn remove_artifact(path: &PathBuf) {
    if path.exists() {
        fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn writer_roundtrip_checks_named_refs_indexes_constants_and_ledger() {
    let path = temp_path("contract-roundtrip");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let registry = json!({
        "schema":"veyra.field_registry/1",
        "fields":[{
            "id":"0x7ffe0001","name":"test.constant","capability":"veyra.cap.conformance_probe/1",
            "domain":"surface","semantic":"x-test.constant/1","persistence":"invariant",
            "storage":{"dtype":"u8","nodata":255},"native_level":0,
            "temporal":{"kind":"static"},"sampling":{"interp":"nearest"},
            "downsample":"mean","compat":"ancillary"
        }]
    });
    let mut body = base_body(&mut writer, "writer-roundtrip", registry);
    body["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    body["capabilities"] = json!([{
        "id":"veyra.cap.conformance_probe/1","params":{},"compat":"ancillary"
    }]);
    body["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":0
    }]);

    let vocab_hash =
        writer.write_json_section("vocab/test.json", br#"{"schema":"test.vocab/1"}"#).unwrap();
    let feature_hash = writer
        .write_json_section("features/test.json", br#"{"schema":"test.feature_table/1"}"#)
        .unwrap();
    body["sections"]["vocab"] = json!([{
        "name":"test.vocab/1","path":"vocab/test.json","hash":vocab_hash.to_string()
    }]);
    body["sections"]["features"] = json!([{
        "name":"test.feature_table/1","path":"features/test.json","hash":feature_hash.to_string()
    }]);

    let field_id = FieldId::new(0x7ffe, 1);
    let index = IndexBlob {
        field_id: field_id.0,
        topology: TopologyTag::DirCube,
        tile_log2: 0,
        entries: vec![IndexEntry {
            level: 0,
            key: DirCube::key(0, 0, 0, 0).unwrap().0,
            value: IndexValue::Const(42),
        }],
    };
    let index_hash = writer.write_index(field_id, &index).unwrap();
    body["indexes"][field_id.to_string()] = json!(index_hash.to_string());

    let (ledger_bytes, ledger_head) = append_extension();
    writer
        .write_ledger("extensions", "extensions/ledger.jsonl", &ledger_bytes, ledger_head)
        .unwrap();
    body["extensions_ledger"] = json!({
        "path":"extensions/ledger.jsonl","hash":ledger_head.to_string()
    });

    let baseline = writer.write_body_json(&serde_json::to_vec(&body).unwrap()).unwrap();
    let opened = open_directory(&path).unwrap();
    assert_eq!(opened.baseline_id(), baseline);
    assert_eq!(verify_directory(&path).unwrap(), baseline);
    assert_eq!(opened.vocabularies()[0].name, "test.vocab/1");
    assert_eq!(opened.feature_tables()[0].name, "test.feature_table/1");
    assert_eq!(opened.index(field_id).unwrap().entries[0].value, IndexValue::Const(42));
    assert_eq!(opened.ledger("extensions"), Some(ledger_bytes.as_slice()));
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_roundtrip_verifies_materialized_star_convex_radius_field() {
    let path = temp_path("star-convex-radius");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let field_id = FieldId::new(0x0100, 1);
    let registry = json!({
        "schema":"veyra.field_registry/1",
        "fields":[{
            "id":field_id.to_string(),"name":"figure.radius_m","capability":"veyra.cap.solid_surface/1",
            "domain":"surface","semantic":"scalar.distance","persistence":"invariant",
            "storage":{"dtype":"u32","scale":"1","offset":"0"},"unit":"m","native_level":0,
            "temporal":{"kind":"static"},"sampling":{"interp":"nearest"},
            "downsample":"mean","compat":"critical"
        }]
    });
    let mut body = base_body(&mut writer, "star-convex-radius", registry);
    body["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    body["figure"] = json!({"kind":"star_convex_radial","radius_field":"figure.radius_m"});
    body["capabilities"] = json!([{
        "id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}
    }]);
    body["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":0
    }]);
    let index = IndexBlob {
        field_id: field_id.0,
        topology: TopologyTag::DirCube,
        tile_log2: 0,
        entries: vec![IndexEntry {
            level: 0,
            key: DirCube::key(0, 0, 0, 0).unwrap().0,
            value: IndexValue::Const(1000),
        }],
    };
    let index_hash = writer.write_index(field_id, &index).unwrap();
    body["indexes"][field_id.to_string()] = json!(index_hash.to_string());
    let baseline = writer.write_body_json(&serde_json::to_vec(&body).unwrap()).unwrap();
    let opened = open_directory(&path).unwrap();
    assert_eq!(opened.baseline_id(), baseline);
    assert_eq!(verify_directory(&path).unwrap(), baseline);
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_refuses_critical_scalar_fields_without_units_before_root_commit() {
    let path = temp_path("critical-scalar-unit-missing");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let registry = json!({
        "schema":"veyra.field_registry/1",
        "fields":[{
            "id":"0x01010001","name":"topography.height_m","capability":"veyra.cap.topography/1",
            "domain":"surface","semantic":"scalar.height","persistence":"invariant",
            "storage":{"dtype":"i16","scale":"0.5","offset":"0"},"native_level":0,
            "temporal":{"kind":"static"},"sampling":{"interp":"bilinear"},
            "downsample":"mean","compat":"critical"
        }]
    });
    let mut body = base_body(&mut writer, "critical-scalar-unit-missing", registry);
    body["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    body["capabilities"] = json!([
        {"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}},
        {"id":"veyra.cap.topography/1","params":{"reference_surface":"datum.mean","domain":"surface"}}
    ]);
    body["reference_surfaces"] = json!([
        {"id":"datum.mean","kind":"sphere","radius_m":"1"},
        {"id":"figure.boundary","kind":"figure_surface"}
    ]);
    body["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":0
    }]);

    assert!(matches!(
        writer.write_body_json(&serde_json::to_vec(&body).unwrap()),
        Err(WriterError::Model(veyra_core::body::ModelError::InvalidField))
    ));
    assert!(!path.join("body.json").exists());
    assert!(!path.join("body.id").exists());
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_refuses_to_commit_star_convex_figures_without_a_registered_radius_field() {
    let path = temp_path("star-convex-missing-radius");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let registry = json!({"schema":"veyra.field_registry/1","fields":[]});
    let mut body = base_body(&mut writer, "star-convex-missing-radius", registry);
    body["figure"] = json!({"kind":"star_convex_radial","radius_field":"missing.radius"});
    body["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    body["capabilities"] = json!([{
        "id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}
    }]);
    body["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":0
    }]);
    assert!(matches!(
        writer.write_body_json(&serde_json::to_vec(&body).unwrap()),
        Err(WriterError::Model(veyra_core::body::ModelError::InvalidFigureField))
    ));
    assert!(!path.join("body.json").exists());
    assert!(!path.join("body.id").exists());
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_refuses_indexes_for_unregistered_or_mismatched_fields_before_root_commit() {
    let path = temp_path("unregistered-index");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let body = base_body(
        &mut writer,
        "unregistered-index",
        json!({
            "schema":"veyra.field_registry/1","fields":[]
        }),
    );
    let field_id = FieldId::new(0x0101, 1);
    let index = IndexBlob {
        field_id: field_id.0,
        topology: TopologyTag::DirCube,
        tile_log2: 0,
        entries: vec![IndexEntry {
            level: 0,
            key: DirCube::key(0, 0, 0, 0).unwrap().0,
            value: IndexValue::Const(0),
        }],
    };
    let index_hash = writer.write_index(field_id, &index).unwrap();
    let mut invalid_body = body;
    invalid_body["indexes"][field_id.to_string()] = json!(index_hash.to_string());
    assert!(matches!(
        writer.write_body_json(&serde_json::to_vec(&invalid_body).unwrap()),
        Err(WriterError::IndexFieldMismatch)
    ));
    assert!(!path.join("body.json").exists());
    assert!(!path.join("body.id").exists());
    drop(writer);
    remove_artifact(&path);

    let path = temp_path("mismatched-index-id");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let index = IndexBlob {
        field_id: FieldId::new(0x0101, 2).0,
        topology: TopologyTag::DirCube,
        tile_log2: 0,
        entries: vec![],
    };
    assert!(matches!(
        writer.write_index(FieldId::new(0x0101, 1), &index),
        Err(WriterError::DuplicateIndex)
    ));
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_refuses_invalid_capability_contracts_before_root_commit() {
    let path = temp_path("invalid-capability");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let mut body = base_body(
        &mut writer,
        "invalid-capability",
        json!({"schema":"veyra.field_registry/1","fields":[]}),
    );
    body["capabilities"] = json!([{
        "id":"veyra.cap.topography/1","params":{"domain":"surface"}
    }]);
    assert!(matches!(
        writer.write_body_json(&serde_json::to_vec(&body).unwrap()),
        Err(WriterError::Model(veyra_core::body::ModelError::InvalidCapabilityParameters(_)))
    ));
    assert!(!path.join("body.json").exists());
    assert!(!path.join("body.id").exists());
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_matches_extension_ledger_path_and_head_and_rejects_conflicts() {
    let (ledger_bytes, head) = append_extension();

    let path = temp_path("ledger-roundtrip");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let mut body = base_body(
        &mut writer,
        "ledger-roundtrip",
        json!({"schema":"veyra.field_registry/1","fields":[]}),
    );
    writer.write_ledger("extensions", "extensions/ledger.jsonl", &ledger_bytes, head).unwrap();
    body["extensions_ledger"] = json!({"path":"extensions/ledger.jsonl","hash":head.to_string()});
    writer.write_body_json(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert_eq!(open_directory(&path).unwrap().ledger("extensions"), Some(ledger_bytes.as_slice()));
    drop(writer);
    remove_artifact(&path);

    for (name, reference_path, reference_head) in [
        ("ledger-wrong-path", "extensions/missing.jsonl", head.to_string()),
        (
            "ledger-wrong-head",
            "extensions/ledger.jsonl",
            "b3:0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
        ),
    ] {
        let path = temp_path(name);
        let mut writer = ArtifactWriter::new(&path).unwrap();
        let mut body =
            base_body(&mut writer, name, json!({"schema":"veyra.field_registry/1","fields":[]}));
        writer.write_ledger("extensions", "extensions/ledger.jsonl", &ledger_bytes, head).unwrap();
        body["extensions_ledger"] = json!({"path":reference_path,"hash":reference_head});
        assert!(matches!(
            writer.write_body_json(&serde_json::to_vec(&body).unwrap()),
            Err(WriterError::InvalidLedger)
        ));
        assert!(!path.join("body.json").exists());
        drop(writer);
        remove_artifact(&path);
    }

    let path = temp_path("ledger-conflict");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    writer.write_ledger("extensions", "extensions/ledger.jsonl", &ledger_bytes, head).unwrap();
    assert!(matches!(
        writer.write_ledger("extensions", "extensions/other.jsonl", &ledger_bytes, head),
        Err(WriterError::InvalidLedger)
    ));
    assert!(matches!(
        writer.write_ledger("other", "extensions/ledger.jsonl", &ledger_bytes, head),
        Err(WriterError::InvalidLedger)
    ));
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_uses_the_shared_literal_artifact_path_grammar() {
    let path = temp_path("paths");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    for invalid in [
        "",
        "/x",
        "a/./b",
        "a//b",
        "../x",
        "a\\b",
        "C:/x",
        "NUL",
        "registry/COM1.txt",
        "bad<name",
        "trailing.",
        "dir /file",
    ] {
        assert!(matches!(writer.write_json_section(invalid, b"{}"), Err(WriterError::InvalidPath)));
    }
    assert!(writer.write_json_section("nested/valid.json", b"{}").is_ok());
    drop(writer);
    remove_artifact(&path);
}

#[test]
fn writer_rejects_case_unicode_and_directory_aliases_before_writing_again() {
    for (left, right) in [
        ("sections/State.json", "sections/state.JSON"),
        ("Data/one.json", "data/ONE.JSON"),
        ("caf\u{00e9}/field.json", "cafe\u{0301}/FIELD.JSON"),
        ("Directory", "directory/child.json"),
    ] {
        for (first, second) in [(left, right), (right, left)] {
            let path = temp_path("path-alias");
            let mut writer = ArtifactWriter::new(&path).unwrap();
            writer.write_json_section(first, br#"{"value":1}"#).unwrap();
            assert!(
                matches!(
                    writer.write_json_section(second, br#"{"value":2}"#),
                    Err(WriterError::DuplicatePath)
                ),
                "accepted {first:?} then {second:?}"
            );
            assert_eq!(fs::read(path.join(first)).unwrap(), br#"{"value":1}"#);
            drop(writer);
            remove_artifact(&path);
        }
    }
}

#[test]
fn writer_reserves_root_files_and_checks_aliases_across_content_kinds() {
    let path = temp_path("root-reservation");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    for reserved in ["BODY.JSON", "Body.Id/child.json"] {
        assert!(matches!(
            writer.write_json_section(reserved, b"{}"),
            Err(WriterError::DuplicatePath)
        ));
    }
    let (ledger_bytes, ledger_head) = append_extension();
    assert!(matches!(
        writer.write_ledger("extensions", "BODY.ID", &ledger_bytes, ledger_head),
        Err(WriterError::DuplicatePath)
    ));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    drop(writer);
    remove_artifact(&path);

    let path = temp_path("index-alias");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    writer.write_json_section("INDEX/0X01010001.IDX", b"{}").unwrap();
    let field_id = FieldId::new(0x0101, 1);
    let index = IndexBlob {
        field_id: field_id.0,
        topology: TopologyTag::DirCube,
        tile_log2: 0,
        entries: vec![],
    };
    assert!(matches!(writer.write_index(field_id, &index), Err(WriterError::DuplicatePath)));
    drop(writer);
    remove_artifact(&path);

    let path = temp_path("ledger-alias");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    writer.write_json_section("EXTENSIONS/LEDGER.JSONL", b"{}").unwrap();
    let (ledger_bytes, ledger_head) = append_extension();
    assert!(matches!(
        writer.write_ledger("extensions", "extensions/ledger.jsonl", &ledger_bytes, ledger_head),
        Err(WriterError::DuplicatePath)
    ));
    drop(writer);
    remove_artifact(&path);

    let path = temp_path("blob-alias");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let canonical =
        CanonicalBlob::new(BlobKind::RasterTile, DType::U8, 1, 1, 1, vec![7]).unwrap().encode();
    let blob_hash = hash::hash(&canonical);
    let hex = blob_hash.text().trim_start_matches("b3:").to_owned();
    let blob_path = format!("BLOBS/{}/{hex}.ZST", &hex[..2]);
    writer.write_json_section(&blob_path, b"{}").unwrap();
    assert!(matches!(writer.write_blob(&canonical), Err(WriterError::DuplicatePath)));
    drop(writer);
    remove_artifact(&path);
}
