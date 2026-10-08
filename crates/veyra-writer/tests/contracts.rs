use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use veyra_core::body::FieldId;
use veyra_core::canon::blob::{BlobKind, CanonicalBlob, DType};
use veyra_core::canon::hash;
use veyra_core::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
use veyra_core::ids::{ObjectAddress, ObjectId, UniverseId};
use veyra_core::sample::{SampleSource, TileRequest, TileView, TimeSel};
use veyra_core::spatial::{DirCube, TileKey};
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
fn rewrite_preserves_unknown_ancillary_artifact_closure() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/worlds/cb6-compat/ancillary");
    let mut root: Value =
        serde_json::from_slice(&fs::read(fixture.join("body.json")).unwrap()).unwrap();
    let mut registry: Value =
        serde_json::from_slice(&fs::read(fixture.join("registry/fields.json")).unwrap()).unwrap();

    root["x-future-root"] = json!({"revision":7,"opaque":["kept",true]});
    root["capabilities"][1]["x-future-capability"] = json!({"configuration":{"mode":"preserve"}});
    registry["fields"][0]["x-future-metadata"]["rewrite-check"] = json!("retained");
    registry["fields"][0]["storage"]["dtype"] = json!("u8");
    root["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    root["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":0
    }]);

    let source_path = temp_path("ancillary-rewrite-source");
    let mut source_writer = ArtifactWriter::new(&source_path).unwrap();
    let registry_hash = source_writer
        .write_json_section("registry/fields.json", &serde_json::to_vec(&registry).unwrap())
        .unwrap();
    root["sections"]["registry"]["hash"] = json!(registry_hash.to_string());

    for relative in ["dynamics/descriptor.json", "dynamics/origin.json"] {
        let bytes = fs::read(fixture.join(relative)).unwrap();
        let hash = source_writer.write_json_section(relative, &bytes).unwrap();
        let reference = if relative.ends_with("descriptor.json") {
            &mut root["dynamics"]["descriptor"]
        } else {
            &mut root["dynamics"]["origin_keyframe"]
        };
        reference["hash"] = json!(hash.to_string());
    }

    let field_id = FieldId::new(0x7ffe, 1);
    let canonical_blob =
        CanonicalBlob::new(BlobKind::RasterTile, DType::U8, 1, 1, 1, vec![42]).unwrap().encode();
    let blob_hash = source_writer.write_blob(&canonical_blob).unwrap();
    let index = IndexBlob {
        field_id: field_id.0,
        topology: TopologyTag::DirCube,
        tile_log2: 0,
        entries: vec![IndexEntry {
            level: 0,
            key: DirCube::key(0, 0, 0, 0).unwrap().0,
            value: IndexValue::Blob(blob_hash),
        }],
    };
    let index_hash = source_writer.write_index(field_id, &index).unwrap();
    root["indexes"][field_id.to_string()] = json!(index_hash.to_string());
    let (ledger_bytes, ledger_head) = append_extension();
    source_writer
        .write_ledger("extensions", "extensions/ledger.jsonl", &ledger_bytes, ledger_head)
        .unwrap();
    root["extensions_ledger"] = json!({
        "path":"extensions/ledger.jsonl","hash":ledger_head.to_string()
    });
    source_writer.write_body_json(&serde_json::to_vec(&root).unwrap()).unwrap();

    let loaded = open_directory(&source_path).unwrap();
    assert_eq!(loaded.root_value()["x-future-root"], root["x-future-root"]);
    assert_eq!(
        loaded.root_value()["capabilities"][1]["x-future-capability"],
        root["capabilities"][1]["x-future-capability"]
    );
    assert_eq!(loaded.fields()[0].extra["x-future-metadata"]["rewrite-check"], "retained");

    let rewritten_path = temp_path("ancillary-rewrite-output");
    let rewritten_baseline = write_loaded_body(&loaded, &rewritten_path);
    let rewritten = open_directory(&rewritten_path).unwrap();
    assert_eq!(rewritten_baseline, loaded.baseline_id());
    assert_body_closure_equal(&loaded, &rewritten);
    assert_eq!(rewritten.fields()[0].extra["x-future-metadata"]["rewrite-check"], "retained");

    drop(source_writer);
    remove_artifact(&source_path);
    remove_artifact(&rewritten_path);
}

fn write_loaded_body(
    body: &veyra_core::io::Body,
    destination: &PathBuf,
) -> veyra_core::ids::Hash32 {
    let root = body.root_value();
    let mut writer = ArtifactWriter::new(destination).unwrap();

    let mut section_refs = std::collections::BTreeMap::new();
    collect_section_refs(&root["sections"], &mut section_refs);
    collect_section_refs(&root["dynamics"], &mut section_refs);
    for (path, expected) in section_refs {
        let section = body.section(&path).unwrap();
        let bytes = serde_json::to_vec(section).unwrap();
        let actual = writer.write_json_section(&path, &bytes).unwrap();
        assert_eq!(actual.to_string(), expected, "section identity changed at {path}");
    }

    let mut blob_hashes = std::collections::BTreeSet::new();
    for field_text in root["indexes"].as_object().unwrap().keys() {
        let field_id = FieldId::parse(field_text).unwrap();
        let index = body.index(field_id).unwrap();
        for entry in &index.entries {
            if let IndexValue::Blob(hash) = entry.value {
                blob_hashes.insert(hash);
            }
        }
    }
    for hash in blob_hashes {
        assert_eq!(writer.write_blob(body.blob(hash).unwrap()).unwrap(), hash);
    }

    for field_text in root["indexes"].as_object().unwrap().keys() {
        let field_id = FieldId::parse(field_text).unwrap();
        let index = body.index(field_id).unwrap();
        let actual = writer.write_index(field_id, index).unwrap();
        assert_eq!(actual.to_string(), root["indexes"][field_text].as_str().unwrap());
    }

    if let Some(reference) = root.get("extensions_ledger") {
        let expected = veyra_core::body::parse_hash(reference["hash"].as_str().unwrap()).unwrap();
        writer
            .write_ledger(
                "extensions",
                reference["path"].as_str().unwrap(),
                body.ledger("extensions").unwrap(),
                expected,
            )
            .unwrap();
    }

    writer.write_body_json(&serde_json::to_vec(root).unwrap()).unwrap()
}

fn collect_section_refs(value: &Value, refs: &mut std::collections::BTreeMap<String, String>) {
    match value {
        Value::Object(object) => {
            if let (Some(path), Some(hash)) = (
                object.get("path").and_then(Value::as_str),
                object.get("hash").and_then(Value::as_str),
            ) {
                assert!(
                    refs.insert(path.to_owned(), hash.to_owned()).is_none_or(|old| old == hash)
                );
            }
            for value in object.values() {
                collect_section_refs(value, refs);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_section_refs(value, refs);
            }
        }
        _ => {}
    }
}

fn assert_body_closure_equal(left: &veyra_core::io::Body, right: &veyra_core::io::Body) {
    assert_eq!(left.baseline_id(), right.baseline_id());
    assert_eq!(left.root_value(), right.root_value());
    let left_root: veyra_core::body::BodyRoot =
        serde_json::from_value(left.root_value().clone()).unwrap();
    let right_root: veyra_core::body::BodyRoot =
        serde_json::from_value(right.root_value().clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&left_root).unwrap(),
        serde_json::to_value(&right_root).unwrap()
    );
    assert_eq!(left_root.extra.get("x-future-root"), right_root.extra.get("x-future-root"));
    assert_eq!(
        left_root.capabilities[1].extra.get("x-future-capability"),
        right_root.capabilities[1].extra.get("x-future-capability")
    );
    assert_eq!(
        serde_json::to_value(left.field_registry()).unwrap(),
        serde_json::to_value(right.field_registry()).unwrap()
    );

    let mut section_refs = std::collections::BTreeMap::new();
    collect_section_refs(left.root_value().get("sections").unwrap(), &mut section_refs);
    collect_section_refs(left.root_value().get("dynamics").unwrap(), &mut section_refs);
    for (path, expected) in section_refs {
        assert_eq!(left.section(&path), right.section(&path), "section changed at {path}");
        let bytes = serde_json::to_vec(left.section(&path).unwrap()).unwrap();
        let canonical = veyra_core::canon::jcs::canonicalize_json(&bytes).unwrap();
        assert_eq!(hash::hash(&canonical).to_string(), expected);
    }

    for field_text in left.root_value()["indexes"].as_object().unwrap().keys() {
        let field_id = FieldId::parse(field_text).unwrap();
        assert_eq!(left.index(field_id), right.index(field_id));
        let index = left.index(field_id).unwrap();
        let canonical = index.encode().unwrap();
        assert_eq!(
            hash::hash(&canonical).to_string(),
            left.root_value()["indexes"][field_text].as_str().unwrap()
        );
        for entry in &index.entries {
            if let IndexValue::Blob(blob_hash) = entry.value {
                assert_eq!(left.blob(blob_hash), right.blob(blob_hash));
                assert_eq!(hash::hash(left.blob(blob_hash).unwrap()), blob_hash);
            }
        }
    }
    if let Some(reference) = left.root_value().get("extensions_ledger") {
        let bytes = left.ledger("extensions").unwrap();
        assert_eq!(Some(bytes), right.ledger("extensions"));
        assert_eq!(
            veyra_core::canon::ledger::verify(bytes).unwrap().hash,
            veyra_core::body::parse_hash(reference["hash"].as_str().unwrap()).unwrap()
        );
    }
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

#[test]
fn capability_declared_slope_and_surface_partition_views_execute_from_their_dependencies() {
    let path = temp_path("derived-view-contract");
    let mut writer = ArtifactWriter::new(&path).unwrap();
    let field_id = FieldId::new(0x0101, 1);
    let registry = json!({
        "schema":"veyra.field_registry/1",
        "fields":[{
            "id":field_id.to_string(),"name":"topography.height_m","capability":"veyra.cap.topography/1",
            "domain":"surface","semantic":"scalar.height","persistence":"invariant",
            "storage":{"dtype":"i16","scale":"1","offset":"0"},"unit":"m",
            "reference":{"surface":"datum"},"native_level":0,
            "temporal":{"kind":"static"},"sampling":{"interp":"bilinear"},
            "downsample":"mean","compat":"critical"
        },{
            "id":"0x7ffe0001","name":"x-future.value","capability":"x-veyra.future/1",
            "domain":"surface","semantic":"x-future.value/1","persistence":"invariant",
            "storage":{"dtype":"u8"},"compat":"ancillary"
        }]
    });
    let mut body = base_body(&mut writer, "derived-view-contract", registry);
    body["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
    body["figure"] = json!({"kind":"sphere","radius_m":"1000"});
    body["reference_surfaces"] = json!([
        {"id":"figure_surface","kind":"figure_surface"},
        {"id":"datum","kind":"sphere","radius_m":"1000"},
        {"id":"ocean_level","kind":"sphere","radius_m":"1001"}
    ]);
    body["capabilities"] = json!([
        {"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}},
        {"id":"veyra.cap.topography/1","params":{"reference_surface":"datum","domain":"surface"}},
        {"id":"veyra.cap.ocean/1","params":{"level_surface":"ocean_level","domain":"surface"}}
    ]);
    body["domains"] = json!([{
        "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
        "vertical":{"kind":"none"},"tile_log2":0,"max_level":0
    }]);
    let entries = (0..6_u8)
        .map(|face| IndexEntry {
            level: 0,
            key: DirCube::key(face, 0, 0, 0).unwrap().0,
            value: IndexValue::Const(0),
        })
        .collect();
    let index =
        IndexBlob { field_id: field_id.0, topology: TopologyTag::DirCube, tile_log2: 0, entries };
    let index_hash = writer.write_index(field_id, &index).unwrap();
    body["indexes"][field_id.to_string()] = json!(index_hash.to_string());
    writer.write_body_json(&serde_json::to_vec(&body).unwrap()).unwrap();
    let opened = open_directory(&path).unwrap();
    let views = opened.views();
    assert!(views.iter().any(|view| view.id == "derived.slope"));
    assert!(views.iter().any(|view| view.id == "derived.land_ocean"));
    let report = opened
        .inspect(
            &veyra_core::sample::Position::Direction(
                veyra_core::spatial::Dir::new(1.0, 0.0, 0.0).unwrap(),
            ),
            veyra_core::sample::LevelSel::Native,
            TimeSel::Static,
        )
        .unwrap();
    assert!(report.fields.iter().any(|field| {
        field.name == "x-future.value" && field.sample.is_none() && field.issue.is_some()
    }));

    let key = TileKey { level: 0, address: DirCube::key(0, 0, 0, 0).unwrap() };
    let slope = opened
        .tile(&TileRequest {
            field: field_id,
            key,
            time: TimeSel::Static,
            halo: 0,
            view: TileView::Derived("derived.slope".to_owned()),
        })
        .unwrap();
    assert_eq!(slope.values, vec![Some(0.0)]);
    assert_eq!(slope.source, SampleSource::Derived);

    let partition = opened
        .tile(&TileRequest {
            field: field_id,
            key,
            time: TimeSel::Static,
            halo: 0,
            view: TileView::Derived("derived.land_ocean".to_owned()),
        })
        .unwrap();
    assert_eq!(partition.values, vec![Some(0.0)]);
    assert_eq!(partition.source, SampleSource::Derived);

    assert!(matches!(
        opened.tile(&TileRequest {
            field: FieldId::new(0x0100, 1),
            key,
            time: TimeSel::Static,
            halo: 0,
            view: TileView::Derived("derived.land_ocean".to_owned()),
        }),
        Err(veyra_core::sample::SampleError::UnknownField)
    ));
    drop(writer);
    remove_artifact(&path);
}
