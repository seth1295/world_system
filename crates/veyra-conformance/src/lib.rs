//! Foundation corpus generation and verification using the actual core and writer APIs.

#![forbid(unsafe_code)]

use core::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use veyra_core::body::{Compatibility, FieldDescriptor, FieldId};
use veyra_core::canon::blob::{BlobKind, CanonicalBlob, DType, decode_zstd_shuffle2};
use veyra_core::canon::hash::{self, body_seed, detail_hash, mix64, subseed};
use veyra_core::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
use veyra_core::canon::jcs;
use veyra_core::ids::{Hash32, ObjectAddress, ObjectId, UniverseId};
use veyra_core::io::{BodyLoader, LoaderError};
use veyra_core::spatial::{CellKey, DirCube, Radial1d, Topology};
use veyra_core::time::UTime;
use veyra_writer::{ArtifactWriter, WriterError, encode_storage_file, open_directory};

const FIELD_CAPABILITY_ID: u16 = 0x7ffe;
const FIELD_ID: FieldId = FieldId::new(FIELD_CAPABILITY_ID, 1);
const FIXTURE_CAPABILITY: &str = "veyra.cap.conformance_probe/1";

/// A conformance result with the stable fixture name and baseline identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureResult {
    /// Fixture or compatibility case name.
    pub name: String,
    /// Short, deterministic result statement.
    pub evidence: String,
}

/// Generates the foundation artifacts and canonical golden vectors at a new output location.
pub fn generate_foundation(
    world_root: impl AsRef<Path>,
    vector_root: impl AsRef<Path>,
) -> Result<(), ConformanceError> {
    let world_root = world_root.as_ref();
    let vector_root = vector_root.as_ref();
    if world_root.join("cb0-addressing/body.json").exists()
        || world_root.join("cb5-hashing/body.json").exists()
        || world_root.join("cb9-minimal-void/body.json").exists()
    {
        verify_foundation(world_root, vector_root)?;
        return Ok(());
    }
    fs::create_dir_all(world_root).map_err(ConformanceError::Io)?;

    let cb0_id = generate_cb0(&world_root.join("cb0-addressing"))?;
    let (cb5_id, cb5_blobs) = generate_cb5(&world_root.join("cb5-hashing"))?;
    let cb9_id = generate_cb9(&world_root.join("cb9-minimal-void"))?;
    generate_cb6(&world_root.join("cb6-compat"), world_root.join("cb5-hashing"), &cb5_blobs)?;

    write_golden_vectors(vector_root, cb0_id, cb5_id, cb9_id)?;
    write_topology_vectors(vector_root)?;
    Ok(())
}

/// Verifies committed foundation artifacts and vectors through the core and writer APIs.
pub fn verify_foundation(
    world_root: impl AsRef<Path>,
    vector_root: impl AsRef<Path>,
) -> Result<Vec<FixtureResult>, ConformanceError> {
    let world_root = world_root.as_ref();
    let vector_root = vector_root.as_ref();
    let mut results = Vec::new();

    let cb0 =
        open_directory(world_root.join("cb0-addressing")).map_err(ConformanceError::Writer)?;
    let topology_vector = vector_root.join("topology/cb0-addressing.json");
    verify_topology_vectors(&topology_vector)?;
    verify_cb0(&cb0, &topology_vector)?;
    results.push(FixtureResult {
        name: "cb0-addressing".to_owned(),
        evidence: format!("baseline={}", cb0.baseline_id()),
    });

    let cb5 = open_directory(world_root.join("cb5-hashing")).map_err(ConformanceError::Writer)?;
    verify_cb5(&cb5)?;
    results.push(FixtureResult {
        name: "cb5-hashing".to_owned(),
        evidence: format!("baseline={}", cb5.baseline_id()),
    });

    verify_cb6(world_root.join("cb6-compat"))?;
    results.push(FixtureResult {
        name: "cb6-compat".to_owned(),
        evidence: "ancillary, minor, critical, major, and tamper cases PASS".to_owned(),
    });

    let cb9 =
        open_directory(world_root.join("cb9-minimal-void")).map_err(ConformanceError::Writer)?;
    if !cb9.fields().is_empty() {
        return Err(ConformanceError::Assertion("cb9 must have zero fields"));
    }
    let root_value = cb9.root_value();
    if root_value["capabilities"].as_array().map_or(0, Vec::len) != 0
        || root_value["domains"].as_array().map_or(0, Vec::len) != 0
    {
        return Err(ConformanceError::Assertion("cb9 must have zero capabilities and domains"));
    }
    results.push(FixtureResult {
        name: "cb9-minimal-void".to_owned(),
        evidence: format!("zero capabilities; baseline={}", cb9.baseline_id()),
    });

    verify_golden_vectors(
        vector_root.join("canon/golden.json"),
        cb0.baseline_id(),
        cb5.baseline_id(),
        cb9.baseline_id(),
    )?;
    Ok(results)
}

fn generate_cb0(path: &Path) -> Result<Hash32, ConformanceError> {
    let mut entries = Vec::new();
    let mut blobs = Vec::new();
    for face in 0..6_u8 {
        let i = u64::from(face % 4);
        let j = u64::from((face * 3) % 4);
        let mut payload = Vec::with_capacity(4 * 4 * 4);
        for tile_j in 0..4_u32 {
            for tile_i in 0..4_u32 {
                let encoded_cell = (u32::from(face) << 4) | (tile_i << 2) | tile_j;
                payload.extend_from_slice(&encoded_cell.to_le_bytes());
            }
        }
        let blob = CanonicalBlob::new(BlobKind::RasterTile, DType::U32, 4, 4, 1, payload)
            .map_err(|_| ConformanceError::Assertion("cb0 raster payload shape"))?;
        let canonical = blob.encode();
        let hash = hash::hash(&canonical);
        let cell =
            DirCube::key(face, i, j, 2).map_err(|_| ConformanceError::Assertion("cb0 cell key"))?;
        let tile =
            DirCube.tile_key(cell, 3).map_err(|_| ConformanceError::Assertion("cb0 tile key"))?;
        entries.push(IndexEntry {
            level: tile.level,
            key: tile.address.0,
            value: IndexValue::Blob(hash),
        });
        blobs.push(canonical);
    }
    let index =
        IndexBlob { field_id: FIELD_ID.0, topology: TopologyTag::DirCube, tile_log2: 3, entries };
    build_artifact(
        path,
        "cb0-addressing",
        vec![addressing_field()],
        ancillary_test_capabilities(),
        Some(dir_cube_domain()),
        vec![index],
        blobs,
        0,
    )
    .map(|result| result.0)
}

fn generate_cb5(path: &Path) -> Result<(Hash32, Vec<Hash32>), ConformanceError> {
    let blob = CanonicalBlob::new(
        BlobKind::RasterTile,
        DType::U32,
        1,
        1,
        1,
        42_u32.to_le_bytes().to_vec(),
    )
    .map_err(|_| ConformanceError::Assertion("cb5 canonical raster"))?;
    let canonical = blob.encode();
    let blob_id = hash::hash(&canonical);
    let entries = (0..4_u8)
        .map(|face| {
            let key = DirCube::key(face, 0, 0, 0).expect("valid face root");
            let value = if face < 2 { IndexValue::Blob(blob_id) } else { IndexValue::Const(7) };
            IndexEntry { level: 0, key: key.0, value }
        })
        .collect();
    let index =
        IndexBlob { field_id: FIELD_ID.0, topology: TopologyTag::DirCube, tile_log2: 3, entries };
    let result = build_artifact(
        path,
        "cb5-hashing",
        vec![addressing_field()],
        ancillary_test_capabilities(),
        Some(dir_cube_domain()),
        vec![index],
        vec![canonical],
        0,
    )?;
    Ok((result.0, result.1))
}

fn generate_cb9(path: &Path) -> Result<Hash32, ConformanceError> {
    build_artifact(
        path,
        "cb9-minimal-void",
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        0,
    )
    .map(|result| result.0)
}

fn generate_cb6(path: &Path, cb5: PathBuf, cb5_blobs: &[Hash32]) -> Result<(), ConformanceError> {
    fs::create_dir_all(path).map_err(ConformanceError::Io)?;

    let ancillary = path.join("ancillary");
    build_artifact(
        &ancillary,
        "cb6-unknown-ancillary",
        vec![ancillary_field()],
        ancillary_test_capabilities(),
        None,
        Vec::new(),
        Vec::new(),
        0,
    )?;

    let minor = path.join("minor-plus-one");
    build_artifact(
        &minor,
        "cb6-minor-plus-one",
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        1,
    )?;

    let critical_capability = path.join("unknown-critical-capability");
    build_artifact(
        &critical_capability,
        "cb6-unknown-critical-capability",
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        0,
    )?;
    mutate_body_root(&critical_capability, |root| {
        root["capabilities"] = json!([{"id":"x-veyra.future/1","compat":"critical","params":{}}]);
    })?;

    let major = path.join("major-plus-one");
    build_artifact(
        &major,
        "cb6-major-plus-one",
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        0,
    )?;
    mutate_body_root(&major, |root| root["format_version"]["major"] = Value::from(2))?;

    let critical_field = path.join("unknown-critical-field");
    build_artifact(
        &critical_field,
        "cb6-unknown-critical-field",
        vec![unknown_critical_field()],
        vec![json!({"id":"veyra.cap.topography/1","params":{}})],
        Some(dir_cube_domain()),
        Vec::new(),
        Vec::new(),
        0,
    )?;
    mutate_registry(&critical_field, |registry| {
        registry["fields"][0]["compat"] = Value::String("critical".to_owned());
        registry["fields"][0]["semantic"] = Value::String("x-unknown.semantic/1".to_owned());
    })?;

    let tampered = path.join("tampered-blob");
    copy_directory(&cb5, &tampered)?;
    let first_blob = cb5_blobs.first().ok_or(ConformanceError::Assertion("cb5 blob ID"))?;
    let hex = first_blob.text().trim_start_matches("b3:").to_owned();
    let file = tampered.join(format!("blobs/{}/{hex}.zst", &hex[..2]));
    let stored = fs::read(&file).map_err(ConformanceError::Io)?;
    let mut canonical = decode_zstd_shuffle2(&stored)
        .map_err(|_| ConformanceError::Assertion("cb5 blob decodes"))?;
    let payload_byte =
        canonical.get_mut(16).ok_or(ConformanceError::Assertion("cb5 blob payload exists"))?;
    *payload_byte ^= 1;
    let stored_tampered = encode_storage_file(&canonical).map_err(ConformanceError::Writer)?;
    fs::write(file, stored_tampered).map_err(ConformanceError::Io)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_artifact(
    path: &Path,
    fixture_name: &str,
    fields: Vec<FieldDescriptor>,
    capabilities: Vec<Value>,
    domain: Option<Value>,
    indexes: Vec<IndexBlob>,
    blobs: Vec<Vec<u8>>,
    minor: u16,
) -> Result<(Hash32, Vec<Hash32>), ConformanceError> {
    let mut writer = ArtifactWriter::new(path).map_err(ConformanceError::Writer)?;
    let registry_value = json!({"schema":"veyra.field_registry/1","fields":fields});
    let registry_bytes = pretty(&registry_value)?;
    let registry_hash = writer
        .write_json_section("registry/fields.json", &registry_bytes)
        .map_err(ConformanceError::Writer)?;
    let descriptor_bytes =
        pretty(&json!({"schema":"veyra.dynamics_descriptor/1","propagator":"opaque-v1"}))?;
    let descriptor_hash = writer
        .write_json_section("dynamics/descriptor.json", &descriptor_bytes)
        .map_err(ConformanceError::Writer)?;
    let origin_bytes = pretty(&json!({"schema":"veyra.dynamics_origin/1","epoch":"0","state":{}}))?;
    let origin_hash = writer
        .write_json_section("dynamics/origin.json", &origin_bytes)
        .map_err(ConformanceError::Writer)?;

    let mut blob_ids = Vec::new();
    for blob in blobs {
        let id = writer.write_blob(&blob).map_err(ConformanceError::Writer)?;
        blob_ids.push(id);
        let duplicate = writer.write_blob(&blob).map_err(ConformanceError::Writer)?;
        if duplicate != id {
            return Err(ConformanceError::Assertion("duplicate blob identity changed"));
        }
    }

    let mut index_refs = serde_json::Map::new();
    for index in indexes {
        let field = FieldId(index.field_id);
        let index_hash = writer.write_index(field, &index).map_err(ConformanceError::Writer)?;
        index_refs.insert(field.to_string(), Value::String(index_hash.to_string()));
    }

    let mut features = vec![
        "veyra.body/1".to_owned(),
        "veyra.canon.jcs/1".to_owned(),
        "veyra.codec.zstd-shuffle2/1".to_owned(),
    ];
    if let Some(domain) = &domain
        && let Some(topology) = domain.get("topology").and_then(Value::as_str)
    {
        features.push(topology.to_owned());
    }
    let domain_list = domain.into_iter().collect::<Vec<_>>();
    let object_id = ObjectId::derive(
        UniverseId::fixture_sentinel(),
        &ObjectAddress::Fixture { name: fixture_name.to_owned() },
    )
    .map_err(ConformanceError::Id)?;
    let body = json!({
        "schema":"veyra.body/1",
        "format_version":{"major":1,"minor":minor},
        "required_features":features,
        "identity":{"object_id":object_id.to_string(),"origin":{"kind":"fixture","name":fixture_name}},
        "classification":{"tags":[]},
        "physical":{"gm_m3_s2":"1","gravity_model":"point_mass"},
        "figure":{"kind":"sphere","radius_m":"1"},
        "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed"}},
        "reference_surfaces":[],
        "dynamics":{
            "descriptor":{"path":"dynamics/descriptor.json","hash":descriptor_hash.to_string()},
            "origin_keyframe":{"path":"dynamics/origin.json","hash":origin_hash.to_string()}
        },
        "capabilities":capabilities,
        "domains":domain_list,
        "codec":"zstd+shuffle2",
        "sections":{"registry":{"path":"registry/fields.json","hash":registry_hash.to_string()}},
        "indexes":index_refs
    });
    let baseline = writer.write_body_json(&pretty(&body)?).map_err(ConformanceError::Writer)?;
    let verified = open_directory(path).map_err(ConformanceError::Writer)?;
    if verified.baseline_id() != baseline {
        return Err(ConformanceError::Assertion(
            "written artifact did not reopen with identical baseline",
        ));
    }
    Ok((baseline, blob_ids))
}

fn addressing_field() -> FieldDescriptor {
    field_descriptor(
        "conformance.cell_address",
        FIXTURE_CAPABILITY,
        "x-cell-address/1",
        "u32",
        Compatibility::Ancillary,
        2,
    )
}

fn ancillary_field() -> FieldDescriptor {
    let mut field = field_descriptor(
        "x-conformance.future_value",
        FIXTURE_CAPABILITY,
        "x-future-value/1",
        "opaque",
        Compatibility::Ancillary,
        0,
    );
    field
        .extra
        .insert("x-future-metadata".to_owned(), json!({"preserved":true,"version":"future"}));
    field
}

fn unknown_critical_field() -> FieldDescriptor {
    field_descriptor(
        "x-conformance.unknown",
        "veyra.cap.topography/1",
        "scalar.test",
        "u8",
        Compatibility::Ancillary,
        0,
    )
}

fn field_descriptor(
    name: &str,
    capability: &str,
    semantic: &str,
    dtype: &str,
    compat: Compatibility,
    native_level: u8,
) -> FieldDescriptor {
    let compat = match compat {
        Compatibility::Critical => "critical",
        Compatibility::Ancillary => "ancillary",
    };
    let id =
        if capability == "veyra.cap.topography/1" { FieldId::new(0x0101, 1) } else { FIELD_ID };
    serde_json::from_value(json!({
        "id":id.to_string(),"name":name,"capability":capability,"domain":"surface","semantic":semantic,
        "persistence":"invariant","storage":{"dtype":dtype,"scale":"1","offset":"0"},
        "native_level":native_level,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},
        "downsample":"mean","compat":compat
    }))
    .expect("well-formed fixture descriptor")
}

fn ancillary_test_capabilities() -> Vec<Value> {
    vec![
        json!({"id":FIXTURE_CAPABILITY,"params":{},"compat":"ancillary"}),
        json!({"id":"x-veyra.future/1","params":{},"compat":"ancillary"}),
    ]
}

fn dir_cube_domain() -> Value {
    json!({"id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":3,"max_level":5})
}

fn verify_cb0(body: &veyra_core::io::Body, vector_path: &Path) -> Result<(), ConformanceError> {
    let index = body.index(FIELD_ID).ok_or(ConformanceError::Assertion("cb0 index missing"))?;
    let vectors: Value =
        serde_json::from_slice(&fs::read(vector_path).map_err(ConformanceError::Io)?)
            .map_err(|_| ConformanceError::Assertion("cb0 addressing vector JSON"))?;
    let cube_vectors =
        vectors["dir_cube"].as_array().ok_or(ConformanceError::Assertion("cb0 cube vectors"))?;
    if index.entries.len() != 6 || cube_vectors.len() != 6 {
        return Err(ConformanceError::Assertion("cb0 must cover six faces"));
    }
    for (face, (entry, expected)) in index.entries.iter().zip(cube_vectors).enumerate() {
        let expected_face =
            expected["face"].as_u64().ok_or(ConformanceError::Assertion("cb0 vector face"))?;
        let expected_i =
            expected["i"].as_u64().ok_or(ConformanceError::Assertion("cb0 vector i"))?;
        let expected_j =
            expected["j"].as_u64().ok_or(ConformanceError::Assertion("cb0 vector j"))?;
        let expected_level =
            expected["level"].as_u64().ok_or(ConformanceError::Assertion("cb0 vector level"))?;
        let expected_key = parse_key(
            expected["key"].as_str().ok_or(ConformanceError::Assertion("cb0 vector key"))?,
        )?;
        let cell = DirCube::key(
            u8::try_from(expected_face)
                .map_err(|_| ConformanceError::Assertion("cb0 face range"))?,
            expected_i,
            expected_j,
            u8::try_from(expected_level)
                .map_err(|_| ConformanceError::Assertion("cb0 level range"))?,
        )
        .map_err(|_| ConformanceError::Assertion("cb0 vector cell"))?;
        if cell.0 != expected_key || u64::from(entry.level) != expected_level {
            return Err(ConformanceError::Assertion("cb0 vector cell or index level mismatch"));
        }
        let expected_tile = DirCube
            .tile_key(cell, index.tile_log2)
            .map_err(|_| ConformanceError::Assertion("cb0 vector tile"))?;
        let key = CellKey(entry.key);
        let (decoded_face, _, _, address_level) =
            DirCube::decode(key).map_err(|_| ConformanceError::Assertion("cb0 key decode"))?;
        if usize::from(decoded_face) != face
            || u64::from(decoded_face) != expected_face
            || address_level != expected_tile.level.saturating_sub(index.tile_log2)
            || key != expected_tile.address
        {
            return Err(ConformanceError::Assertion("cb0 face or level mismatch"));
        }
        let canonical = match entry.value {
            IndexValue::Blob(blob_hash) => {
                body.blob(blob_hash).ok_or(ConformanceError::Assertion("cb0 blob missing"))?
            }
            IndexValue::Const(_) => {
                return Err(ConformanceError::Assertion("cb0 address values must be stored"));
            }
        };
        let blob = CanonicalBlob::decode(canonical)
            .map_err(|_| ConformanceError::Assertion("cb0 blob header"))?;
        if blob.kind != BlobKind::RasterTile
            || blob.dim_i != 4
            || blob.dim_j != 4
            || blob.slices != 1
        {
            return Err(ConformanceError::Assertion("cb0 raster dimensions match its tile"));
        }
        let (chunks, remainder) = blob.payload.as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(ConformanceError::Assertion("cb0 payload is aligned"));
        }
        let values: Vec<u32> = chunks.iter().map(|bytes| u32::from_le_bytes(*bytes)).collect();
        for tile_j in 0..4_u32 {
            for tile_i in 0..4_u32 {
                let expected_value = (u32::from(decoded_face) << 4) | (tile_i << 2) | tile_j;
                let offset = usize::try_from(tile_j * 4 + tile_i)
                    .map_err(|_| ConformanceError::Assertion("cb0 tile offset range"))?;
                if values[offset] != expected_value {
                    return Err(ConformanceError::Assertion(
                        "cb0 stored cell addresses disagree with the raster tile positions",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn write_topology_vectors(vector_root: &Path) -> Result<(), ConformanceError> {
    let cube = (0..6_u8)
        .map(|face| {
            let i = u64::from(face % 4);
            let j = u64::from((face * 3) % 4);
            let key = DirCube::key(face, i, j, 2).expect("valid addressing fixture cell");
            json!({"face":face,"i":i,"j":j,"level":2,"key":format_key(key.0)})
        })
        .collect::<Vec<_>>();
    let radial_topology = Radial1d::default();
    let radial = [(0_u8, 0_u64), (1, 0), (1, 1), (3, 5), (5, 31)]
        .into_iter()
        .map(|(level, index)| {
            let key = Radial1d::key(level, index).expect("valid radial fixture cell");
            let parent = radial_topology.parent(key).expect("valid radial parent");
            let children = if level < 30 {
                radial_topology.children(key).expect("valid radial children")
            } else {
                Vec::new()
            };
            json!({
                "level":level,
                "index":index,
                "key":format_key(key.0),
                "parent":parent.map(|key| format_key(key.0)),
                "children":children.into_iter().map(|child| format_key(child.0)).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let vectors = json!({"schema":"veyra.conformance.addressing_vectors/1","dir_cube":cube,"radial_1d":radial});
    write_new(vector_root.join("topology/cb0-addressing.json"), &pretty(&vectors)?)
}

fn verify_topology_vectors(path: &Path) -> Result<(), ConformanceError> {
    let vectors: Value = serde_json::from_slice(&fs::read(path).map_err(ConformanceError::Io)?)
        .map_err(|_| ConformanceError::Assertion("addressing vector JSON"))?;
    if vectors["schema"] != "veyra.conformance.addressing_vectors/1" {
        return Err(ConformanceError::Assertion("addressing vector schema"));
    }
    for row in vectors["dir_cube"].as_array().ok_or(ConformanceError::Assertion("cube vectors"))? {
        let face =
            u8::try_from(row["face"].as_u64().ok_or(ConformanceError::Assertion("cube face"))?)
                .map_err(|_| ConformanceError::Assertion("cube face range"))?;
        let i = row["i"].as_u64().ok_or(ConformanceError::Assertion("cube i"))?;
        let j = row["j"].as_u64().ok_or(ConformanceError::Assertion("cube j"))?;
        let level =
            u8::try_from(row["level"].as_u64().ok_or(ConformanceError::Assertion("cube level"))?)
                .map_err(|_| ConformanceError::Assertion("cube level range"))?;
        let expected =
            parse_key(row["key"].as_str().ok_or(ConformanceError::Assertion("cube key"))?)?;
        let actual = DirCube::key(face, i, j, level)
            .map_err(|_| ConformanceError::Assertion("cube cell construction"))?;
        if actual.0 != expected
            || DirCube::decode(actual).map(|value| (value.0, value.1, value.2, value.3))
                != Ok((face, i, j, level))
        {
            return Err(ConformanceError::Assertion("direction-cube addressing vector mismatch"));
        }
    }
    let topology = Radial1d::default();
    for row in
        vectors["radial_1d"].as_array().ok_or(ConformanceError::Assertion("radial vectors"))?
    {
        let level =
            u8::try_from(row["level"].as_u64().ok_or(ConformanceError::Assertion("radial level"))?)
                .map_err(|_| ConformanceError::Assertion("radial level range"))?;
        let index = row["index"].as_u64().ok_or(ConformanceError::Assertion("radial index"))?;
        let key = Radial1d::key(level, index)
            .map_err(|_| ConformanceError::Assertion("radial cell construction"))?;
        let expected =
            parse_key(row["key"].as_str().ok_or(ConformanceError::Assertion("radial key"))?)?;
        let parent =
            topology.parent(key).map_err(|_| ConformanceError::Assertion("radial parent"))?;
        let children =
            topology.children(key).map_err(|_| ConformanceError::Assertion("radial children"))?;
        let expected_parent = row["parent"].as_str().map(parse_key).transpose()?;
        let expected_children: Vec<u64> = row["children"]
            .as_array()
            .ok_or(ConformanceError::Assertion("radial children vector"))?
            .iter()
            .map(|child| {
                parse_key(child.as_str().ok_or(ConformanceError::Assertion("radial child key"))?)
            })
            .collect::<Result<_, _>>()?;
        if key.0 != expected
            || parent.map(|value| value.0) != expected_parent
            || children.into_iter().map(|value| value.0).collect::<Vec<_>>() != expected_children
        {
            return Err(ConformanceError::Assertion("radial addressing vector mismatch"));
        }
    }
    Ok(())
}

fn verify_cb5(body: &veyra_core::io::Body) -> Result<(), ConformanceError> {
    let index = body.index(FIELD_ID).ok_or(ConformanceError::Assertion("cb5 index missing"))?;
    if index.entries.len() != 4 {
        return Err(ConformanceError::Assertion("cb5 index entry count"));
    }
    let mut blob_ids = std::collections::BTreeSet::new();
    let mut constants = 0;
    for entry in &index.entries {
        match entry.value {
            IndexValue::Blob(blob_id) => {
                blob_ids.insert(blob_id);
                if body.blob(blob_id).is_none() {
                    return Err(ConformanceError::Assertion("cb5 blob missing"));
                }
            }
            IndexValue::Const(7) => constants += 1,
            IndexValue::Const(_) => {
                return Err(ConformanceError::Assertion("cb5 constant differs"));
            }
        }
    }
    if blob_ids.len() != 1 || constants != 2 {
        return Err(ConformanceError::Assertion("cb5 dedup or constant entries differ"));
    }
    Ok(())
}

fn verify_cb6(path: PathBuf) -> Result<(), ConformanceError> {
    let ancillary = open_directory(path.join("ancillary")).map_err(ConformanceError::Writer)?;
    if ancillary.fields().len() != 1 || ancillary.fields()[0].compat != Compatibility::Ancillary {
        return Err(ConformanceError::Assertion("unknown ancillary field was not preserved"));
    }
    if ancillary.fields()[0].extra.get("x-future-metadata")
        != Some(&json!({"preserved":true,"version":"future"}))
    {
        return Err(ConformanceError::Assertion(
            "unknown ancillary field metadata was not preserved",
        ));
    }
    if !ancillary.root_value()["capabilities"].as_array().is_some_and(|capabilities| {
        capabilities.iter().any(|item| item["id"] == "x-veyra.future/1")
    }) {
        return Err(ConformanceError::Assertion("unknown ancillary capability was not preserved"));
    }
    let minor = open_directory(path.join("minor-plus-one")).map_err(ConformanceError::Writer)?;
    if minor.root_value()["format_version"]["minor"] != 1 {
        return Err(ConformanceError::Assertion("minor-plus-one body did not open"));
    }
    expect_begin_error(path.join("unknown-critical-capability"), |error| {
        matches!(
            error,
            LoaderError::Model(veyra_core::body::ModelError::UnknownCriticalCapability(_))
        )
    })?;
    expect_begin_error(path.join("major-plus-one"), |error| {
        matches!(
            error,
            LoaderError::Model(veyra_core::body::ModelError::UnsupportedMajorVersion(2))
        )
    })?;
    expect_finish_error(path.join("unknown-critical-field"), |error| {
        matches!(
            error,
            WriterError::Loader(LoaderError::Model(
                veyra_core::body::ModelError::UnknownCriticalSemantic(_)
            ))
        )
    })?;
    expect_finish_error(path.join("tampered-blob"), |error| {
        matches!(error, WriterError::Loader(LoaderError::HashMismatch(_)))
    })?;
    Ok(())
}

fn expect_begin_error(
    path: PathBuf,
    predicate: impl FnOnce(&LoaderError) -> bool,
) -> Result<(), ConformanceError> {
    let bytes = fs::read(path.join("body.json")).map_err(ConformanceError::Io)?;
    let error = BodyLoader::begin(&bytes)
        .err()
        .ok_or(ConformanceError::Assertion("expected body begin refusal"))?;
    if !predicate(&error) {
        return Err(ConformanceError::Unexpected(format!("unexpected begin error: {error}")));
    }
    Ok(())
}

fn expect_finish_error(
    path: PathBuf,
    predicate: impl FnOnce(&WriterError) -> bool,
) -> Result<(), ConformanceError> {
    let result = open_directory(path);
    let error = result.err().ok_or(ConformanceError::Assertion("expected body finish refusal"))?;
    if !predicate(&error) {
        return Err(ConformanceError::Unexpected(format!("unexpected open error: {error}")));
    }
    Ok(())
}

fn write_golden_vectors(
    root: &Path,
    cb0: Hash32,
    cb5: Hash32,
    cb9: Hash32,
) -> Result<(), ConformanceError> {
    fs::create_dir_all(root.join("canon")).map_err(ConformanceError::Io)?;
    let json_input = br#"{"z":-2,"a":"value"}"#;
    let jcs_bytes = jcs::canonicalize_json(json_input).map_err(ConformanceError::Jcs)?;
    let blob = CanonicalBlob::new(BlobKind::RasterTile, DType::I16, 2, 1, 1, vec![1, 0, 2, 0])
        .map_err(|_| ConformanceError::Assertion("golden VYB1 blob"))?;
    let blob_bytes = blob.encode();
    let index = IndexBlob {
        field_id: FieldId::new(0x0101, 1).0,
        topology: TopologyTag::DirCube,
        tile_log2: 3,
        entries: vec![IndexEntry { level: 0, key: 1_u64 << 60, value: IndexValue::Const(-7) }],
    };
    let index_bytes = index.encode().map_err(|_| ConformanceError::Assertion("golden index"))?;
    let universe = UniverseId([7; 32]);
    let address = ObjectAddress::Fixture { name: "cb0-addressing".to_owned() };
    let object_id = ObjectId::derive(universe, &address).map_err(ConformanceError::Id)?;
    let system_address = ObjectAddress::SystemSeed {
        region: veyra_core::ids::RegionKey { level: 0, ix: -1, iy: 64, iz: 0 },
        slot: 300,
    };
    let body_address =
        ObjectAddress::BodyInSystem { system: ObjectId([0x11; 16]), role: 2, ordinal: 128 };
    let free_address = ObjectAddress::FreeObject {
        region: veyra_core::ids::RegionKey { level: 0, ix: -2, iy: 3, iz: -4 },
        slot: 1,
    };
    let seed_id = ObjectId([0x42; 16]);
    let seed = body_seed(seed_id);
    let detail_seed = subseed(seed, "refine/example");
    let (ledger_bytes, ledger_head) = veyra_core::canon::ledger::append(
        &[],
        "created",
        json!({"value":"one"}),
        UTime::from_nanos(0),
    )
    .map_err(|_| ConformanceError::Assertion("golden ledger"))?;
    let vectors = json!({
        "schema":"veyra.conformance.canonical_vectors/1",
        "jcs":{"input":"{\"z\":-2,\"a\":\"value\"}","canonical":String::from_utf8(jcs_bytes.clone()).unwrap(),"hash":hash::hash(&jcs_bytes).to_string()},
        "blob":{"hex":hex(&blob_bytes),"hash":hash::hash(&blob_bytes).to_string()},
        "index":{"hex":hex(&index_bytes),"hash":hash::hash(&index_bytes).to_string()},
        "object_id":{"universe_hex":hex(&universe.0),"address_hex":hex(&address.encode().map_err(ConformanceError::Id)?),"id":object_id.to_string()},
        "address_kinds":{
            "system_seed":hex(&system_address.encode().map_err(ConformanceError::Id)?),
            "body_in_system":hex(&body_address.encode().map_err(ConformanceError::Id)?),
            "free_object":hex(&free_address.encode().map_err(ConformanceError::Id)?),
            "fixture":hex(&address.encode().map_err(ConformanceError::Id)?)
        },
        "seed":{"object_id":seed_id.to_string(),"body_seed":format!("{seed:016x}"),"subseed":format!("{detail_seed:016x}"),"mix64_zero":format!("{:016x}",mix64(0)),"detail_hash":format!("{:016x}",detail_hash(detail_seed,0x1234,2))},
        "fixture_baselines":{"cb0-addressing":cb0.to_string(),"cb5-hashing":cb5.to_string(),"cb9-minimal-void":cb9.to_string()},
        "ledger":{"bytes_hex":hex(&ledger_bytes),"head":ledger_head.to_string()}
    });
    write_new(root.join("canon/golden.json"), &pretty(&vectors)?)
}

fn verify_golden_vectors(
    path: PathBuf,
    cb0: Hash32,
    cb5: Hash32,
    cb9: Hash32,
) -> Result<(), ConformanceError> {
    let vectors: Value = serde_json::from_slice(&fs::read(path).map_err(ConformanceError::Io)?)
        .map_err(|_| ConformanceError::Assertion("golden vectors JSON"))?;
    if vectors["fixture_baselines"]["cb0-addressing"] != cb0.to_string()
        || vectors["fixture_baselines"]["cb5-hashing"] != cb5.to_string()
        || vectors["fixture_baselines"]["cb9-minimal-void"] != cb9.to_string()
    {
        return Err(ConformanceError::Assertion("fixture BaselineId vector mismatch"));
    }
    let canonical = jcs::canonicalize_json(
        vectors["jcs"]["input"]
            .as_str()
            .ok_or(ConformanceError::Assertion("JCS vector input"))?
            .as_bytes(),
    )
    .map_err(ConformanceError::Jcs)?;
    if String::from_utf8(canonical.clone()).map_err(|_| ConformanceError::Assertion("JCS UTF-8"))?
        != vectors["jcs"]["canonical"]
        || hash::hash(&canonical).to_string() != vectors["jcs"]["hash"]
    {
        return Err(ConformanceError::Assertion("JCS canonical bytes or hash vector mismatch"));
    }

    let blob = CanonicalBlob::new(BlobKind::RasterTile, DType::I16, 2, 1, 1, vec![1, 0, 2, 0])
        .map_err(|_| ConformanceError::Assertion("golden blob parameters"))?
        .encode();
    let index = IndexBlob {
        field_id: FieldId::new(0x0101, 1).0,
        topology: TopologyTag::DirCube,
        tile_log2: 3,
        entries: vec![IndexEntry { level: 0, key: 1_u64 << 60, value: IndexValue::Const(-7) }],
    }
    .encode()
    .map_err(|_| ConformanceError::Assertion("golden index parameters"))?;
    if hex(&blob) != vectors["blob"]["hex"]
        || hash::hash(&blob).to_string() != vectors["blob"]["hash"]
        || hex(&index) != vectors["index"]["hex"]
        || hash::hash(&index).to_string() != vectors["index"]["hash"]
    {
        return Err(ConformanceError::Assertion("blob or index byte vector mismatch"));
    }

    let universe_bytes = unhex(
        vectors["object_id"]["universe_hex"]
            .as_str()
            .ok_or(ConformanceError::Assertion("object vector universe"))?,
    )?;
    let universe = UniverseId(
        universe_bytes
            .try_into()
            .map_err(|_| ConformanceError::Assertion("universe ID vector width"))?,
    );
    let address_bytes = unhex(
        vectors["object_id"]["address_hex"]
            .as_str()
            .ok_or(ConformanceError::Assertion("object vector address"))?,
    )?;
    let address = ObjectAddress::decode(&address_bytes)
        .map_err(|_| ConformanceError::Assertion("object address vector decode"))?;
    let derived_object = ObjectId::derive(universe, &address).map_err(ConformanceError::Id)?;
    if derived_object.to_string() != vectors["object_id"]["id"] {
        return Err(ConformanceError::Assertion("ObjectId derivation vector mismatch"));
    }
    let address_vectors = [
        (
            "system_seed",
            ObjectAddress::SystemSeed {
                region: veyra_core::ids::RegionKey { level: 0, ix: -1, iy: 64, iz: 0 },
                slot: 300,
            },
        ),
        (
            "body_in_system",
            ObjectAddress::BodyInSystem { system: ObjectId([0x11; 16]), role: 2, ordinal: 128 },
        ),
        (
            "free_object",
            ObjectAddress::FreeObject {
                region: veyra_core::ids::RegionKey { level: 0, ix: -2, iy: 3, iz: -4 },
                slot: 1,
            },
        ),
        ("fixture", ObjectAddress::Fixture { name: "cb0-addressing".to_owned() }),
    ];
    for (name, expected) in address_vectors {
        if hex(&expected.encode().map_err(ConformanceError::Id)?) != vectors["address_kinds"][name]
        {
            return Err(ConformanceError::Assertion("ObjectAddress encoding vector mismatch"));
        }
    }

    let seed_id = ObjectId::parse(
        vectors["seed"]["object_id"]
            .as_str()
            .ok_or(ConformanceError::Assertion("seed object ID"))?,
    )
    .map_err(|_| ConformanceError::Assertion("seed object ID form"))?;
    let seed = body_seed(seed_id);
    let detail_seed = subseed(seed, "refine/example");
    if format!("{seed:016x}") != vectors["seed"]["body_seed"]
        || format!("{detail_seed:016x}") != vectors["seed"]["subseed"]
        || format!("{:016x}", mix64(0)) != vectors["seed"]["mix64_zero"]
        || format!("{:016x}", detail_hash(detail_seed, 0x1234, 2)) != vectors["seed"]["detail_hash"]
    {
        return Err(ConformanceError::Assertion("seed derivation vector mismatch"));
    }

    let (ledger_bytes, ledger_head) = veyra_core::canon::ledger::append(
        &[],
        "created",
        json!({"value":"one"}),
        UTime::from_nanos(0),
    )
    .map_err(|_| ConformanceError::Assertion("ledger vector append"))?;
    if hex(&ledger_bytes) != vectors["ledger"]["bytes_hex"]
        || ledger_head.to_string() != vectors["ledger"]["head"]
    {
        return Err(ConformanceError::Assertion("ledger byte or head vector mismatch"));
    }
    Ok(())
}

fn mutate_body_root(path: &Path, update: impl FnOnce(&mut Value)) -> Result<(), ConformanceError> {
    let body_path = path.join("body.json");
    let mut root: Value =
        serde_json::from_slice(&fs::read(&body_path).map_err(ConformanceError::Io)?)
            .map_err(|_| ConformanceError::Assertion("generated root JSON"))?;
    update(&mut root);
    let bytes = pretty(&root)?;
    let id = hash::hash(&jcs::canonicalize_json(&bytes).map_err(ConformanceError::Jcs)?);
    fs::write(&body_path, &bytes).map_err(ConformanceError::Io)?;
    fs::write(path.join("body.id"), format!("bas:{id}\n")).map_err(ConformanceError::Io)?;
    Ok(())
}

fn mutate_registry(path: &Path, update: impl FnOnce(&mut Value)) -> Result<(), ConformanceError> {
    let body_path = path.join("body.json");
    let mut root: Value =
        serde_json::from_slice(&fs::read(&body_path).map_err(ConformanceError::Io)?)
            .map_err(|_| ConformanceError::Assertion("generated root JSON"))?;
    let registry_path = root["sections"]["registry"]["path"]
        .as_str()
        .ok_or(ConformanceError::Assertion("registry path"))?;
    let registry_file = path.join(registry_path);
    let mut registry: Value =
        serde_json::from_slice(&fs::read(&registry_file).map_err(ConformanceError::Io)?)
            .map_err(|_| ConformanceError::Assertion("registry JSON"))?;
    update(&mut registry);
    let registry_bytes = pretty(&registry)?;
    let registry_hash =
        hash::hash(&jcs::canonicalize_json(&registry_bytes).map_err(ConformanceError::Jcs)?);
    root["sections"]["registry"]["hash"] = Value::String(registry_hash.to_string());
    let root_bytes = pretty(&root)?;
    let baseline = hash::hash(&jcs::canonicalize_json(&root_bytes).map_err(ConformanceError::Jcs)?);
    fs::write(registry_file, registry_bytes).map_err(ConformanceError::Io)?;
    fs::write(&body_path, root_bytes).map_err(ConformanceError::Io)?;
    fs::write(path.join("body.id"), format!("bas:{baseline}\n")).map_err(ConformanceError::Io)?;
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), ConformanceError> {
    fs::create_dir(target).map_err(ConformanceError::Io)?;
    for entry in fs::read_dir(source).map_err(ConformanceError::Io)? {
        let entry = entry.map_err(ConformanceError::Io)?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_directory(&from, &to)?;
        } else {
            fs::copy(from, to).map_err(ConformanceError::Io)?;
        }
    }
    Ok(())
}

fn pretty(value: &Value) -> Result<Vec<u8>, ConformanceError> {
    serde_json::to_vec_pretty(value)
        .map_err(|_| ConformanceError::Assertion("fixture JSON serialization"))
}

fn write_new(path: PathBuf, bytes: &[u8]) -> Result<(), ConformanceError> {
    let parent = path.parent().ok_or(ConformanceError::Assertion("vector path parent"))?;
    fs::create_dir_all(parent).map_err(ConformanceError::Io)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(ConformanceError::Io)?;
    use std::io::Write;
    file.write_all(bytes).map_err(ConformanceError::Io)?;
    file.sync_all().map_err(ConformanceError::Io)?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    result
}

fn unhex(text: &str) -> Result<Vec<u8>, ConformanceError> {
    if !text.len().is_multiple_of(2) {
        return Err(ConformanceError::Assertion("hex vector length"));
    }
    let (pairs, remainder) = text.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(ConformanceError::Assertion("hex vector length"));
    }
    pairs
        .iter()
        .map(|pair| {
            let high = nibble(pair[0]).ok_or(ConformanceError::Assertion("hex digit"))?;
            let low = nibble(pair[1]).ok_or(ConformanceError::Assertion("hex digit"))?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn format_key(key: u64) -> String {
    format!("0x{key:016x}")
}

fn parse_key(text: &str) -> Result<u64, ConformanceError> {
    let hex = text.strip_prefix("0x").ok_or(ConformanceError::Assertion("cell key vector form"))?;
    u64::from_str_radix(hex, 16).map_err(|_| ConformanceError::Assertion("cell key vector digits"))
}

/// Fixture generation or conformance assertion failure.
#[derive(Debug)]
pub enum ConformanceError {
    /// Filesystem input/output failed.
    Io(std::io::Error),
    /// Writer operation failed.
    Writer(WriterError),
    /// JCS validation failed.
    Jcs(veyra_core::canon::jcs::JcsError),
    /// Identifier construction or encoding failed.
    Id(veyra_core::ids::IdError),
    /// An expected artifact property was false.
    Assertion(&'static str),
    /// A result differed from its expected error or vector.
    Unexpected(String),
}

impl fmt::Display for ConformanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Writer(error) => error.fmt(formatter),
            Self::Jcs(error) => error.fmt(formatter),
            Self::Id(error) => error.fmt(formatter),
            Self::Assertion(message) => formatter.write_str(message),
            Self::Unexpected(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ConformanceError {}
