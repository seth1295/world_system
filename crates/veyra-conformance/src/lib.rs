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
    let foundation_ready = ["cb0-addressing", "cb5-hashing", "cb9-minimal-void"]
        .iter()
        .all(|name| world_root.join(name).join("body.json").exists());
    if !foundation_ready {
        fs::create_dir_all(world_root).map_err(ConformanceError::Io)?;
        let cb0_id = generate_cb0(&world_root.join("cb0-addressing"))?;
        let (cb5_id, cb5_blobs) = generate_cb5(&world_root.join("cb5-hashing"))?;
        let cb9_id = generate_cb9(&world_root.join("cb9-minimal-void"))?;
        generate_cb6(&world_root.join("cb6-compat"), world_root.join("cb5-hashing"), &cb5_blobs)?;
        write_golden_vectors(vector_root, cb0_id, cb5_id, cb9_id)?;
        write_topology_vectors(vector_root)?;
    }
    generate_stage4(world_root)?;
    verify_foundation(world_root, vector_root)?;
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
    results.extend(verify_stage4(world_root)?);
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

fn generate_stage4(world_root: &Path) -> Result<(), ConformanceError> {
    if !world_root.join("cb1-seams/body.json").exists() {
        generate_cb1(&world_root.join("cb1-seams"))?;
    }
    if !world_root.join("cb2-categories/body.json").exists() {
        generate_cb2(&world_root.join("cb2-categories"))?;
    }
    if !world_root.join("cb7-star1d/body.json").exists() {
        generate_cb7(&world_root.join("cb7-star1d"))?;
    }
    if !world_root.join("cb8-rock/body.json").exists() {
        generate_cb8(&world_root.join("cb8-rock"))?;
    }
    let corpus_root =
        world_root.parent().ok_or(ConformanceError::Assertion("conformance world root parent"))?;
    let query_path = corpus_root.join("queries/stage4.jsonl");
    if !query_path.exists() {
        let queries = stage4_queries();
        let lines = queries
            .iter()
            .map(|query| {
                serde_json::to_string(query)
                    .map_err(|_| ConformanceError::Assertion("Stage 4 query JSON"))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        write_new(query_path.clone(), format!("{lines}\n").as_bytes())?;
    }
    let expected_path = corpus_root.join("expected/stage4.jsonl");
    if !expected_path.exists() {
        let output = run_stage4_queries(world_root, &query_path)?;
        write_new(expected_path, format!("{}\n", output.join("\n")).as_bytes())?;
    }
    Ok(())
}

fn generate_cb1(path: &Path) -> Result<(), ConformanceError> {
    let mut analytic_field = stage4_field(
        FieldId::new(0x7ffe, 1),
        "conformance.seam_scalar",
        FIXTURE_CAPABILITY,
        "surface",
        "scalar.temperature",
        "f32",
        "1",
        "0",
        Some("K"),
        "bilinear",
        "mean",
        2,
        "ancillary",
        None,
        Some(json!({"group":"Conformance","label":"Analytic seam field","legend":"continuous"})),
    );
    analytic_field["sampling"]["above_native"] = json!("refine");
    let mut periodic_field = stage4_field(
        FieldId::new(0x7ffe, 3),
        "conformance.periodic_signal",
        FIXTURE_CAPABILITY,
        "surface",
        "scalar.temperature",
        "u16",
        "1",
        "0",
        Some("K"),
        "bilinear",
        "mean",
        2,
        "ancillary",
        None,
        Some(json!({"group":"Conformance","label":"Periodic signal","legend":"continuous"})),
    );
    periodic_field["temporal"] = json!({"kind":"periodic_slices","count":3,"period_ref":"rotation.period","origin_ref":"rotation.epoch","reduce_default":"mean"});
    periodic_field["sampling"]["above_native"] = json!("smooth_only");
    let mut checker_field = stage4_field(
        FieldId::new(0x7ffe, 2),
        "conformance.face_checker",
        FIXTURE_CAPABILITY,
        "surface",
        "category",
        "u8",
        "1",
        "0",
        None,
        "nearest",
        "mode_lowest_tiebreak",
        2,
        "ancillary",
        Some(json!({"nodata":255})),
        Some(json!({"group":"Conformance","label":"Face checker","legend":"vocab_counts"})),
    );
    checker_field["sampling"]["above_native"] = json!("inherit");
    for field in [&mut analytic_field, &mut periodic_field, &mut checker_field] {
        field["sampling"].as_object_mut().unwrap().remove("below_native");
    }
    let fields = vec![analytic_field, checker_field, periodic_field];
    let (analytic, analytic_blobs) =
        dir_cube_index(FieldId::new(0x7ffe, 1), DType::F32, 2, 2, |face, i, j| {
            let key = DirCube::key(face, i, j, 2).expect("fixture cell");
            let direction = DirCube.cell_center(key).expect("fixture direction");
            let value = (direction.x() + 2.0 * direction.y() + 3.0 * direction.z()) as f32;
            value.to_bits().to_le_bytes().to_vec()
        })?;
    let (mut checker, mut checker_blobs) =
        dir_cube_index(FieldId::new(0x7ffe, 2), DType::U8, 2, 2, |face, i, j| {
            vec![if face == 0 && i == 0 && j == 0 {
                255
            } else {
                1 + ((face + i as u8 + j as u8) % 3)
            }]
        })?;
    let constant = checker
        .entries
        .iter_mut()
        .find(|entry| entry.key >> 61 == 5)
        .ok_or(ConformanceError::Assertion("cb1 constant face entry"))?;
    constant.value = IndexValue::Const(2);
    checker_blobs.pop();
    let (periodic, periodic_blobs) =
        dir_cube_sliced_index(FieldId::new(0x7ffe, 3), DType::U16, 2, 2, 3, |_, _, _, slice| {
            (10_u16 + slice * 10).to_le_bytes().to_vec()
        })?;
    let mut domain = dir_cube_domain_2();
    domain["max_level"] = json!(3);
    build_stage4_artifact(
        path,
        "cb1-seams",
        fields,
        vec![json!({"id":FIXTURE_CAPABILITY,"params":{},"compat":"ancillary"})],
        vec![domain],
        json!({"kind":"sphere","radius_m":"1"}),
        vec![],
        vec![],
        vec![analytic, checker, periodic],
        [analytic_blobs, checker_blobs, periodic_blobs].concat(),
        json!({"fixture":"cube seams and corners"}),
    )?;
    Ok(())
}

fn generate_cb2(path: &Path) -> Result<(), ConformanceError> {
    let field = stage4_field(
        FieldId::new(0x0105, 1),
        "surface_material.class",
        "veyra.cap.surface_material/1",
        "surface",
        "category",
        "u8",
        "1",
        "0",
        None,
        "nearest",
        "mode_lowest_tiebreak",
        2,
        "critical",
        Some(json!({"nodata":255,"vocab":"surface_material.class/1"})),
        Some(json!({"group":"Surface material","label":"Material class","legend":"vocab_counts"})),
    );
    let (index, blobs) = dir_cube_pyramid_index(
        FieldId::new(0x0105, 1),
        DType::U8,
        2,
        2,
        "mode_lowest_tiebreak",
        Some(255),
        |face, i, j| {
            vec![if face == 0 && i == 0 && j == 0 {
                255
            } else {
                ((face + i as u8 + j as u8) % 3) + 1
            }]
        },
    )?;
    let vocab = json!({"schema":"veyra.vocab/1","id":"surface_material.class/1","values":[
        {"id":1,"label":"Class A"},{"id":2,"label":"Class B"},{"id":3,"label":"Class C"}
    ]});
    build_stage4_artifact(
        path,
        "cb2-categories",
        vec![field],
        vec![
            json!({"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}}),
            json!({"id":"veyra.cap.surface_material/1","params":{"domain":"surface"}}),
        ],
        vec![dir_cube_domain_2()],
        json!({"kind":"sphere","radius_m":"1"}),
        vec![json!({"id":"figure_surface","kind":"figure_surface"})],
        vec![(
            "surface_material.class/1".to_owned(),
            "vocab/surface_material.class.json".to_owned(),
            vocab,
        )],
        vec![index],
        blobs,
        json!({"fixture":"categorical values, ties, and nodata"}),
    )?;
    Ok(())
}

fn generate_cb7(path: &Path) -> Result<(), ConformanceError> {
    let templates = [
        (1, "stellar.density", "scalar.density", "u32", "0.01", "kg/m3", "Stellar density"),
        (2, "stellar.temperature", "scalar.temperature", "u32", "0.01", "K", "Stellar temperature"),
        (3, "stellar.pressure", "scalar.pressure", "u32", "0.01", "Pa", "Stellar pressure"),
        (
            4,
            "stellar.hydrogen_fraction",
            "scalar.fraction",
            "u16",
            "0.0001",
            "1",
            "Hydrogen fraction",
        ),
    ];
    let mut fields = Vec::new();
    let mut indexes = Vec::new();
    let mut blobs = Vec::new();
    for (local_id, name, semantic, dtype, scale, unit, label) in templates {
        let field_id = FieldId::new(0x0130, local_id);
        fields.push(stage4_field(
            field_id,
            name,
            "veyra.cap.stellar_structure/1",
            "interior",
            semantic,
            dtype,
            scale,
            "0",
            Some(unit),
            "linear",
            "mean",
            4,
            "critical",
            None,
            Some(json!({"group":"Stellar structure","label":label,"legend":"continuous"})),
        ));
        let dtype_value = if dtype == "u16" { DType::U16 } else { DType::U32 };
        let raw_values = (0..16_u64)
            .map(|shell| match local_id {
                1 => 10_000 + shell as i64 * 100,
                2 => 100_000 + shell as i64 * 2_000,
                3 => 1_000_000 + shell as i64 * 100_000,
                _ => 7_000 - shell as i64 * 100,
            })
            .collect();
        let (index, field_blobs) =
            radial_pyramid_index(field_id, dtype_value, 2, 4, "mean", None, raw_values)?;
        indexes.push(index);
        blobs.extend(field_blobs);
    }
    build_stage4_artifact(
        path,
        "cb7-star1d",
        fields,
        vec![json!({"id":"veyra.cap.stellar_structure/1","params":{"domain":"interior"}})],
        vec![
            json!({"id":"interior","topology":"veyra.topo.radial_1d/1","frame":"body_fixed","vertical":{"kind":"radius","extent_m":"10"},"tile_log2":2,"max_level":4}),
        ],
        json!({"kind":"radial_profile_sphere","extent_m":"10"}),
        vec![json!({"id":"photosphere","kind":"sphere","radius_m":"10"})],
        vec![],
        indexes,
        blobs,
        json!({"class":"star","fixture":"radial stellar interior"}),
    )?;
    Ok(())
}

fn generate_cb8(path: &Path) -> Result<(), ConformanceError> {
    let radius_id = FieldId::new(0x0100, 1);
    let material_id = FieldId::new(0x0105, 1);
    let thermal_id = FieldId::new(0x0106, 1);
    let mut fields = vec![
        stage4_field(
            radius_id,
            "figure.radius_m",
            "veyra.cap.solid_surface/1",
            "surface",
            "scalar.distance",
            "u32",
            "1",
            "0",
            Some("m"),
            "bilinear",
            "mean",
            2,
            "critical",
            None,
            Some(json!({"group":"Solid surface","label":"Radius","legend":"continuous"})),
        ),
        stage4_field(
            material_id,
            "surface_material.class",
            "veyra.cap.surface_material/1",
            "surface",
            "category",
            "u8",
            "1",
            "0",
            None,
            "nearest",
            "mode_lowest_tiebreak",
            2,
            "critical",
            Some(json!({"nodata":255,"vocab":"surface_material.class/1"})),
            Some(
                json!({"group":"Surface material","label":"Material class","legend":"vocab_counts"}),
            ),
        ),
        stage4_field(
            thermal_id,
            "thermal_state.surface_temperature",
            "veyra.cap.thermal_state/1",
            "surface",
            "scalar.temperature",
            "i16",
            "0.1",
            "0",
            Some("K"),
            "bilinear",
            "mean",
            2,
            "critical",
            None,
            Some(
                json!({"group":"Thermal state","label":"Surface temperature","legend":"continuous"}),
            ),
        ),
    ];
    for field in &mut fields {
        field["sampling"].as_object_mut().unwrap().remove("below_native");
    }
    let (radius_index, mut blobs) = dir_cube_index(radius_id, DType::U32, 2, 2, |face, i, j| {
        (1000_u32 + u32::from(face) * 100 + i as u32 * 20 + j as u32 * 7).to_le_bytes().to_vec()
    })?;
    let (material_index, material_blobs) =
        dir_cube_index(material_id, DType::U8, 2, 2, |face, i, j| {
            vec![((face + i as u8 + j as u8) % 4) + 1]
        })?;
    let (thermal_index, thermal_blobs) =
        dir_cube_index(thermal_id, DType::I16, 2, 2, |face, i, j| {
            (2500_i16 + i as i16 * 15 + j as i16 * 7 + i16::from(face) * 10).to_le_bytes().to_vec()
        })?;
    blobs.extend(material_blobs);
    blobs.extend(thermal_blobs);
    let vocab = json!({"schema":"veyra.vocab/1","id":"surface_material.class/1","values":[
        {"id":1,"label":"Rock A"},{"id":2,"label":"Rock B"},{"id":3,"label":"Rock C"},{"id":4,"label":"Rock D"}
    ]});
    build_stage4_artifact(
        path,
        "cb8-rock",
        fields,
        vec![
            json!({"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}}),
            json!({"id":"veyra.cap.surface_material/1","params":{"domain":"surface"}}),
            json!({"id":"veyra.cap.thermal_state/1","params":{"domain":"surface"}}),
        ],
        vec![dir_cube_domain_2()],
        json!({"kind":"star_convex_radial","radius_field":"figure.radius_m"}),
        vec![json!({"id":"figure_surface","kind":"figure_surface"})],
        vec![(
            "surface_material.class/1".to_owned(),
            "vocab/surface_material.class.json".to_owned(),
            vocab,
        )],
        vec![radius_index, material_index, thermal_index],
        blobs,
        json!({"class":"irregular_rock","fixture":"star-convex non-spherical surface"}),
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn stage4_field(
    id: FieldId,
    name: &str,
    capability: &str,
    domain: &str,
    semantic: &str,
    dtype: &str,
    scale: &str,
    offset: &str,
    unit: Option<&str>,
    interpolation: &str,
    downsample: &str,
    native_level: u8,
    compat: &str,
    storage_extra: Option<Value>,
    display: Option<Value>,
) -> Value {
    let mut storage = json!({"dtype":dtype,"scale":scale,"offset":offset});
    if let Some(extra) = storage_extra.and_then(|value| value.as_object().cloned()) {
        let object = storage.as_object_mut().expect("storage object");
        object.extend(extra);
    }
    let mut field = json!({
        "id":id.to_string(),"name":name,"capability":capability,"domain":domain,
        "semantic":semantic,"persistence":"invariant","storage":storage,
        "native_level":native_level,"temporal":{"kind":"static"},
        "sampling":{"interp":interpolation,"below_native":"pyramid"},
        "downsample":downsample,"compat":compat
    });
    let object = field.as_object_mut().expect("field object");
    if let Some(unit) = unit {
        object.insert("unit".to_owned(), json!(unit));
    }
    if let Some(display) = display {
        object.insert("display".to_owned(), display);
    }
    field
}

fn dir_cube_index(
    field_id: FieldId,
    dtype: DType,
    tile_log2: u8,
    level: u8,
    mut pixel: impl FnMut(u8, u64, u64) -> Vec<u8>,
) -> Result<(IndexBlob, Vec<Vec<u8>>), ConformanceError> {
    dir_cube_sliced_index(field_id, dtype, tile_log2, level, 1, |face, i, j, _| pixel(face, i, j))
}

fn dir_cube_sliced_index(
    field_id: FieldId,
    dtype: DType,
    tile_log2: u8,
    level: u8,
    slices: u16,
    mut pixel: impl FnMut(u8, u64, u64, u16) -> Vec<u8>,
) -> Result<(IndexBlob, Vec<Vec<u8>>), ConformanceError> {
    let edge = 1_u64 << level.min(tile_log2);
    let mut entries = Vec::new();
    let mut blobs = Vec::new();
    for face in 0..6_u8 {
        let mut payload = Vec::new();
        for slice in 0..slices {
            for j in 0..edge {
                for i in 0..edge {
                    payload.extend(pixel(face, i, j, slice));
                }
            }
        }
        let blob = CanonicalBlob::new(
            BlobKind::RasterTile,
            dtype,
            u16::try_from(edge).map_err(|_| ConformanceError::Assertion("cube tile width"))?,
            u16::try_from(edge).map_err(|_| ConformanceError::Assertion("cube tile height"))?,
            slices,
            payload,
        )
        .map_err(|_| ConformanceError::Assertion("cube raster blob"))?;
        let canonical = blob.encode();
        let blob_id = hash::hash(&canonical);
        let cell = DirCube::key(face, 0, 0, level)
            .map_err(|_| ConformanceError::Assertion("cube index tile cell"))?;
        let tile = DirCube
            .tile_key(cell, tile_log2)
            .map_err(|_| ConformanceError::Assertion("cube index tile key"))?;
        entries.push(IndexEntry {
            level: tile.level,
            key: tile.address.0,
            value: IndexValue::Blob(blob_id),
        });
        blobs.push(canonical);
    }
    Ok((
        IndexBlob { field_id: field_id.0, topology: TopologyTag::DirCube, tile_log2, entries },
        blobs,
    ))
}

fn dir_cube_pyramid_index(
    field_id: FieldId,
    dtype: DType,
    tile_log2: u8,
    native_level: u8,
    operator: &str,
    nodata: Option<i64>,
    pixel: impl FnMut(u8, u64, u64) -> Vec<u8>,
) -> Result<(IndexBlob, Vec<Vec<u8>>), ConformanceError> {
    let (_, native_blobs) = dir_cube_index(field_id, dtype, tile_log2, native_level, pixel)?;
    let mut entries = Vec::new();
    let mut blobs = Vec::new();
    for face in 0..6_u8 {
        let mut canonical = native_blobs[usize::from(face)].clone();
        let mut level = native_level;
        loop {
            let decoded = CanonicalBlob::decode(&canonical)
                .map_err(|_| ConformanceError::Assertion("pyramid input raster"))?;
            let cell = DirCube::key(face, 0, 0, level)
                .map_err(|_| ConformanceError::Assertion("pyramid cell"))?;
            let tile = DirCube
                .tile_key(cell, tile_log2)
                .map_err(|_| ConformanceError::Assertion("pyramid tile key"))?;
            let hash = hash::hash(&canonical);
            entries.push(IndexEntry { level, key: tile.address.0, value: IndexValue::Blob(hash) });
            blobs.push(canonical.clone());
            if level == 0 {
                break;
            }
            let parent = veyra_writer::PyramidBuilder::downsample_tile(
                TopologyTag::DirCube,
                dtype,
                operator,
                nodata,
                &[decoded],
            )
            .map_err(ConformanceError::Writer)?;
            canonical = parent.encode();
            level -= 1;
        }
    }
    entries.sort_by_key(|entry| (entry.level, entry.key));
    Ok((
        IndexBlob { field_id: field_id.0, topology: TopologyTag::DirCube, tile_log2, entries },
        blobs,
    ))
}

fn radial_pyramid_index(
    field_id: FieldId,
    dtype: DType,
    tile_log2: u8,
    native_level: u8,
    operator: &str,
    nodata: Option<i64>,
    native_values: Vec<i64>,
) -> Result<(IndexBlob, Vec<Vec<u8>>), ConformanceError> {
    let native_count = 1_usize << native_level;
    if native_values.len() != native_count {
        return Err(ConformanceError::Assertion("radial pyramid native value count"));
    }
    let mut levels: Vec<Vec<i64>> = vec![Vec::new(); usize::from(native_level) + 1];
    levels[usize::from(native_level)] = native_values;
    for level in (1..=native_level).rev() {
        let children = &levels[usize::from(level)];
        let (pairs, remainder) = children.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(ConformanceError::Assertion("radial pyramid child pairs"));
        }
        let mut parents = Vec::with_capacity(pairs.len());
        for pair in pairs {
            parents.push(
                veyra_writer::PyramidBuilder::reduce_group(dtype, operator, nodata, pair)
                    .map_err(ConformanceError::Writer)?,
            );
        }
        levels[usize::from(level - 1)] = parents;
    }
    let mut entries = Vec::new();
    let mut blobs = Vec::new();
    for level in 0..=native_level {
        let count = 1_u64 << level;
        let edge = 1_u64 << level.min(tile_log2);
        for start in (0..count).step_by(usize::try_from(edge).unwrap()) {
            let mut payload = Vec::new();
            for raw in &levels[usize::from(level)][start as usize..(start + edge) as usize] {
                payload.extend(encode_integer_pixel(dtype, *raw)?);
            }
            let blob = CanonicalBlob::new(
                BlobKind::RasterTile,
                dtype,
                u16::try_from(edge)
                    .map_err(|_| ConformanceError::Assertion("radial tile width"))?,
                1,
                1,
                payload,
            )
            .map_err(|_| ConformanceError::Assertion("radial raster blob"))?;
            let canonical = blob.encode();
            let blob_id = hash::hash(&canonical);
            let cell = Radial1d::key(level, start)
                .map_err(|_| ConformanceError::Assertion("radial index tile cell"))?;
            let tile = Radial1d::default()
                .tile_key(cell, tile_log2)
                .map_err(|_| ConformanceError::Assertion("radial index tile key"))?;
            entries.push(IndexEntry {
                level: tile.level,
                key: tile.address.0,
                value: IndexValue::Blob(blob_id),
            });
            blobs.push(canonical);
        }
    }
    entries.sort_by_key(|entry| (entry.level, entry.key));
    Ok((
        IndexBlob { field_id: field_id.0, topology: TopologyTag::Radial1d, tile_log2, entries },
        blobs,
    ))
}

fn encode_integer_pixel(dtype: DType, value: i64) -> Result<Vec<u8>, ConformanceError> {
    match dtype {
        DType::U8 => u8::try_from(value)
            .map(|value| vec![value])
            .map_err(|_| ConformanceError::Assertion("radial u8 pyramid range")),
        DType::I8 => i8::try_from(value)
            .map(|value| vec![value as u8])
            .map_err(|_| ConformanceError::Assertion("radial i8 pyramid range")),
        DType::U16 => u16::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| ConformanceError::Assertion("radial u16 pyramid range")),
        DType::I16 => i16::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| ConformanceError::Assertion("radial i16 pyramid range")),
        DType::U32 => u32::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| ConformanceError::Assertion("radial u32 pyramid range")),
        DType::I32 => i32::try_from(value)
            .map(|value| value.to_le_bytes().to_vec())
            .map_err(|_| ConformanceError::Assertion("radial i32 pyramid range")),
        DType::F32 | DType::Raw => Err(ConformanceError::Assertion("radial pyramid dtype")),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_stage4_artifact(
    path: &Path,
    fixture_name: &str,
    fields: Vec<Value>,
    capabilities: Vec<Value>,
    domains: Vec<Value>,
    figure: Value,
    reference_surfaces: Vec<Value>,
    vocabularies: Vec<(String, String, Value)>,
    indexes: Vec<IndexBlob>,
    blobs: Vec<Vec<u8>>,
    classification: Value,
) -> Result<Hash32, ConformanceError> {
    let mut writer = ArtifactWriter::new(path).map_err(ConformanceError::Writer)?;
    let registry = json!({"schema":"veyra.field_registry/1","fields":fields});
    let registry_hash = writer
        .write_json_section("registry/fields.json", &pretty(&registry)?)
        .map_err(ConformanceError::Writer)?;
    let descriptor_hash = writer
        .write_json_section(
            "dynamics/descriptor.json",
            &pretty(&json!({"schema":"veyra.dynamics_descriptor/1","type":"opaque-v1"}))?,
        )
        .map_err(ConformanceError::Writer)?;
    let origin_hash = writer
        .write_json_section(
            "dynamics/origin.json",
            &pretty(&json!({"schema":"veyra.dynamics_origin/1","epoch":"0","state":{}}))?,
        )
        .map_err(ConformanceError::Writer)?;
    let mut vocab_refs = Vec::new();
    for (name, section_path, document) in vocabularies {
        let section_hash = writer
            .write_json_section(&section_path, &pretty(&document)?)
            .map_err(ConformanceError::Writer)?;
        vocab_refs.push(json!({"name":name,"path":section_path,"hash":section_hash.to_string()}));
    }
    for blob in blobs {
        writer.write_blob(&blob).map_err(ConformanceError::Writer)?;
    }
    let mut index_refs = serde_json::Map::new();
    for index in indexes {
        let field_id = FieldId(index.field_id);
        let index_hash = writer.write_index(field_id, &index).map_err(ConformanceError::Writer)?;
        index_refs.insert(field_id.to_string(), Value::String(index_hash.to_string()));
    }
    let mut required_features = std::collections::BTreeSet::from([
        "veyra.body/1".to_owned(),
        "veyra.canon.jcs/1".to_owned(),
        "veyra.codec.zstd-shuffle2/1".to_owned(),
    ]);
    for domain in &domains {
        if let Some(topology) = domain.get("topology").and_then(Value::as_str) {
            required_features.insert(topology.to_owned());
        }
    }
    for capability in &capabilities {
        if let Some(id) = capability.get("id").and_then(Value::as_str)
            && id != FIXTURE_CAPABILITY
        {
            required_features.insert(id.to_owned());
        }
    }
    let object_id = ObjectId::derive(
        UniverseId::fixture_sentinel(),
        &ObjectAddress::Fixture { name: fixture_name.to_owned() },
    )
    .map_err(ConformanceError::Id)?;
    let body = json!({
        "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
        "required_features":required_features.into_iter().collect::<Vec<_>>(),
        "identity":{"object_id":object_id.to_string(),"origin":{"kind":"fixture","name":fixture_name}},
        "classification":classification,"physical":{"gm_m3_s2":"1","gravity_model":"point_mass"},
        "figure":figure,
        "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed","rotation":{"kind":"uniform","period_s":"86400","epoch":"0","orientation_q_at_epoch":["1","0","0","0"],"relative_to":"universe_inertial"}}},
        "reference_surfaces":reference_surfaces,
        "dynamics":{"descriptor":{"path":"dynamics/descriptor.json","hash":descriptor_hash.to_string()},"origin_keyframe":{"path":"dynamics/origin.json","hash":origin_hash.to_string()}},
        "capabilities":capabilities,"domains":domains,"codec":"zstd+shuffle2",
        "sections":{"registry":{"path":"registry/fields.json","hash":registry_hash.to_string()},"vocab":vocab_refs},
        "indexes":index_refs
    });
    let baseline = writer.write_body_json(&pretty(&body)?).map_err(ConformanceError::Writer)?;
    let reopened = open_directory(path).map_err(ConformanceError::Writer)?;
    if reopened.baseline_id() != baseline {
        return Err(ConformanceError::Assertion("Stage 4 artifact did not round trip"));
    }
    Ok(baseline)
}

fn dir_cube_domain_2() -> Value {
    json!({"id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":2,"max_level":2})
}

fn stage4_queries() -> Vec<Value> {
    let cell = |face, i, j| format_key(DirCube::key(face, i, j, 2).expect("Stage 4 query cell").0);
    let cube_tile = DirCube
        .tile_key(DirCube::key(0, 0, 0, 2).expect("Stage 4 geometry cell"), 2)
        .expect("Stage 4 geometry tile");
    let face_five_tile = DirCube
        .tile_key(DirCube::key(5, 0, 0, 2).expect("Stage 4 face cell"), 2)
        .expect("Stage 4 face tile");
    let radial_tile = Radial1d::default()
        .tile_key(Radial1d::key(4, 0).expect("Stage 4 radial cell"), 2)
        .expect("Stage 4 radial tile");
    let mut queries = vec![
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0001","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":2,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0001","position":{"kind":"direction","xyz":[1,1,0]},"level":2,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0001","position":{"kind":"direction","xyz":[1,1,1]},"level":2,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0002","position":{"kind":"cell","domain":"surface","key":cell(5,0,0)},"level":2,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0002","position":{"kind":"cell","domain":"surface","key":cell(0,0,1)},"level":3,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0003","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":3,"time":"slice:0"}),
        json!({"fixture":"cb2-categories","op":"sample","field":"0x01050001","position":{"kind":"cell","domain":"surface","key":cell(0,0,0)},"level":2,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0003","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":2,"time":"mean"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0003","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":2,"time":"phase:0.5"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0003","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":2,"time":"slice:2"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0003","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":2,"time":"min"}),
        json!({"fixture":"cb1-seams","op":"sample","field":"0x7ffe0003","position":{"kind":"cell","domain":"surface","key":cell(0,1,1)},"level":2,"time":"max"}),
        json!({"fixture":"cb1-seams","op":"tile","field":"0x7ffe0003","key":{"level":2,"address":format_key(DirCube::key(0,0,0,0).unwrap().0)},"time":"slice:1","view":"raw","halo":1}),
        json!({"fixture":"cb1-seams","op":"tile","field":"0x7ffe0001","key":{"level":face_five_tile.level,"address":format_key(face_five_tile.address.0)},"time":"static","view":"derived:topology.cube_face","halo":0}),
        json!({"fixture":"cb1-seams","op":"tile","field":"0x7ffe0001","key":{"level":cube_tile.level,"address":format_key(cube_tile.address.0)},"time":"static","view":"derived:topology.tile_level","halo":0}),
        json!({"fixture":"cb1-seams","op":"tile","field":"0x7ffe0001","key":{"level":cube_tile.level,"address":format_key(cube_tile.address.0)},"time":"static","view":"derived:topology.axial_latitude","halo":0}),
        json!({"fixture":"cb7-star1d","op":"tile","field":"0x01300001","key":{"level":radial_tile.level,"address":format_key(radial_tile.address.0)},"time":"static","view":"derived:topology.radial_profile","halo":0}),
        json!({"fixture":"cb2-categories","op":"sample","field":"0x01050001","position":{"kind":"cell","domain":"surface","key":cell(0,0,0)},"level":2,"time":"static"}),
        json!({"fixture":"cb2-categories","op":"sample","field":"0x01050001","position":{"kind":"cell","domain":"surface","key":cell(0,0,1)},"level":2,"time":"static"}),
        json!({"fixture":"cb2-categories","op":"sample","field":"0x01050001","position":{"kind":"cell","domain":"surface","key":cell(0,0,0)},"level":1,"time":"static"}),
        json!({"fixture":"cb2-categories","op":"histogram","field":"0x01050001","bins":4,"level":2,"time":"static"}),
        json!({"fixture":"cb1-seams","op":"stats","field":"0x7ffe0001","level":2,"time":"static"}),
        json!({"fixture":"cb7-star1d","op":"sample","field":"0x01300001","position":{"kind":"radius","r_m":5},"level":4,"time":"static"}),
        json!({"fixture":"cb7-star1d","op":"sample","field":"0x01300001","position":{"kind":"radius","r_m":5},"level":2,"time":"static"}),
        json!({"fixture":"cb7-star1d","op":"stats","field":"0x01300001","level":"native","time":"static"}),
        json!({"fixture":"cb7-star1d","op":"views"}),
        json!({"fixture":"cb8-rock","op":"geometry","domain":"surface","tile":{"level":cube_tile.level,"address":format_key(cube_tile.address.0)},"grid_n":4}),
        json!({"fixture":"cb8-rock","op":"views"}),
        json!({"fixture":"cb9-minimal-void","op":"views"}),
    ];
    for face in 0..6_u8 {
        for (s, t) in [(0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)] {
            let direction =
                DirCube.direction_at_face_st(face, s, t).expect("cube edge query direction");
            queries.push(json!({
                "fixture":"cb1-seams","op":"sample","field":"0x7ffe0001",
                "position":{"kind":"direction","xyz":[direction.x(),direction.y(),direction.z()]},
                "level":2,"time":"static"
            }));
        }
    }
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                queries.push(json!({
                    "fixture":"cb1-seams","op":"sample","field":"0x7ffe0001",
                    "position":{"kind":"direction","xyz":[x,y,z]},
                    "level":2,"time":"static"
                }));
            }
        }
    }
    for face in 0..6_u8 {
        for j in 0..6_u32 {
            for i in 0..6_u32 {
                let s = (f64::from(i) + 0.5) / 6.0;
                let t = (f64::from(j) + 0.5) / 6.0;
                let direction =
                    DirCube.direction_at_face_st(face, s, t).expect("cube grid query direction");
                queries.push(json!({
                    "fixture":"cb1-seams","op":"sample","field":"0x7ffe0001",
                    "position":{"kind":"direction","xyz":[direction.x(),direction.y(),direction.z()]},
                    "level":2,"time":"static"
                }));
            }
        }
    }
    queries
}

fn verify_stage4(world_root: &Path) -> Result<Vec<FixtureResult>, ConformanceError> {
    let cb1 = open_directory(world_root.join("cb1-seams")).map_err(ConformanceError::Writer)?;
    let scalar_field = FieldId::new(0x7ffe, 1);
    for face in 0..6_u8 {
        for i in 0..4_u64 {
            for j in 0..4_u64 {
                let key = DirCube::key(face, i, j, 2)
                    .map_err(|_| ConformanceError::Assertion("cb1 cell key"))?;
                let direction = DirCube
                    .cell_center(key)
                    .map_err(|_| ConformanceError::Assertion("cb1 cell center"))?;
                let expected =
                    f64::from((direction.x() + 2.0 * direction.y() + 3.0 * direction.z()) as f32);
                let sample = cb1
                    .sample(&veyra_core::sample::SampleQuery {
                        field: scalar_field,
                        pos: veyra_core::sample::Position::Cell {
                            domain: "surface".to_owned(),
                            key,
                        },
                        level: veyra_core::sample::LevelSel::Exact(2),
                        time: veyra_core::sample::TimeSel::Static,
                    })
                    .map_err(|_| ConformanceError::Assertion("cb1 analytic center sample"))?;
                if sample.value.is_none_or(|value| (value - expected).abs() > 1.0e-6) {
                    return Err(ConformanceError::Assertion(
                        "cb1 analytic sample differs from its stored function",
                    ));
                }
            }
        }
    }
    let mut edge_samples = 0;
    for face in 0..6_u8 {
        for (s, t) in [(0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)] {
            let direction = DirCube
                .direction_at_face_st(face, s, t)
                .map_err(|_| ConformanceError::Assertion("cb1 edge direction"))?;
            let sample = cb1
                .sample(&veyra_core::sample::SampleQuery {
                    field: scalar_field,
                    pos: veyra_core::sample::Position::Direction(direction),
                    level: veyra_core::sample::LevelSel::Exact(2),
                    time: veyra_core::sample::TimeSel::Static,
                })
                .map_err(|_| ConformanceError::Assertion("cb1 edge sample"))?;
            if sample.value.is_none_or(|value| !value.is_finite()) {
                return Err(ConformanceError::Assertion("cb1 edge sample is not finite"));
            }
            edge_samples += 1;
        }
    }
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                let direction = veyra_core::spatial::Dir::new(x, y, z)
                    .map_err(|_| ConformanceError::Assertion("cb1 corner direction"))?;
                let sample = cb1
                    .sample(&veyra_core::sample::SampleQuery {
                        field: scalar_field,
                        pos: veyra_core::sample::Position::Direction(direction),
                        level: veyra_core::sample::LevelSel::Exact(2),
                        time: veyra_core::sample::TimeSel::Static,
                    })
                    .map_err(|_| ConformanceError::Assertion("cb1 corner sample"))?;
                if sample.value.is_none_or(|value| !value.is_finite()) {
                    return Err(ConformanceError::Assertion("cb1 corner sample is not finite"));
                }
            }
        }
    }
    let periodic_cell =
        DirCube::key(0, 1, 1, 2).map_err(|_| ConformanceError::Assertion("cb1 periodic cell"))?;
    for (time, expected, source) in [
        (veyra_core::sample::TimeSel::Static, 20.0, veyra_core::sample::SampleSource::TimeReduced),
        (veyra_core::sample::TimeSel::Mean, 20.0, veyra_core::sample::SampleSource::TimeReduced),
        (veyra_core::sample::TimeSel::Phase(0.5), 20.0, veyra_core::sample::SampleSource::Stored),
        (veyra_core::sample::TimeSel::Slice(2), 30.0, veyra_core::sample::SampleSource::Stored),
        (veyra_core::sample::TimeSel::Min, 10.0, veyra_core::sample::SampleSource::TimeReduced),
        (veyra_core::sample::TimeSel::Max, 30.0, veyra_core::sample::SampleSource::TimeReduced),
    ] {
        let sample = cb1
            .sample(&veyra_core::sample::SampleQuery {
                field: FieldId::new(0x7ffe, 3),
                pos: veyra_core::sample::Position::Cell {
                    domain: "surface".to_owned(),
                    key: periodic_cell,
                },
                level: veyra_core::sample::LevelSel::Exact(2),
                time,
            })
            .map_err(|_| ConformanceError::Assertion("cb1 periodic sample"))?;
        if sample.value != Some(expected) || sample.source != source {
            return Err(ConformanceError::Assertion("cb1 periodic selection or source reporting"));
        }
    }
    let periodic_tile_key = DirCube.tile_key(DirCube::key(0, 0, 0, 2).unwrap(), 2).unwrap();
    let raw_slice = cb1
        .tile(&veyra_core::sample::TileRequest {
            field: FieldId::new(0x7ffe, 3),
            key: periodic_tile_key,
            time: veyra_core::sample::TimeSel::Slice(0),
            halo: 0,
            view: veyra_core::sample::TileView::Raw,
        })
        .map_err(|_| ConformanceError::Assertion("cb1 raw periodic tile"))?;
    let reduced_tile = cb1
        .tile(&veyra_core::sample::TileRequest {
            field: FieldId::new(0x7ffe, 3),
            key: periodic_tile_key,
            time: veyra_core::sample::TimeSel::Mean,
            halo: 0,
            view: veyra_core::sample::TileView::TimeReduce,
        })
        .map_err(|_| ConformanceError::Assertion("cb1 reduced periodic tile"))?;
    if raw_slice
        .values
        .first()
        .and_then(|value| *value)
        .is_none_or(|value| (value - 10.0).abs() > 1.0e-12)
        || reduced_tile
            .values
            .first()
            .and_then(|value| *value)
            .is_none_or(|value| (value - 20.0).abs() > 1.0e-12)
        || !matches!(
            cb1.tile(&veyra_core::sample::TileRequest {
                field: FieldId::new(0x7ffe, 3),
                key: periodic_tile_key,
                time: veyra_core::sample::TimeSel::Static,
                halo: 0,
                view: veyra_core::sample::TileView::Raw,
            }),
            Err(veyra_core::sample::SampleError::UnsupportedSelection)
        )
    {
        return Err(ConformanceError::Assertion("cb1 raw versus reduced tile time selection"));
    }
    for level in [veyra_core::sample::LevelSel::Exact(3), veyra_core::sample::LevelSel::Canonical] {
        if !matches!(
            cb1.sample(&veyra_core::sample::SampleQuery {
                field: scalar_field,
                pos: veyra_core::sample::Position::Direction(
                    veyra_core::spatial::Dir::new(1.0, 0.0, 0.0).unwrap(),
                ),
                level,
                time: veyra_core::sample::TimeSel::Static,
            }),
            Err(veyra_core::sample::SampleError::UnsupportedRefinement)
        ) {
            return Err(ConformanceError::Assertion(
                "unimplemented refinement must fail explicitly",
            ));
        }
    }
    let stats = cb1
        .stats(
            scalar_field,
            veyra_core::sample::LevelSel::Exact(2),
            veyra_core::sample::TimeSel::Static,
        )
        .map_err(|_| ConformanceError::Assertion("cb1 weighted statistics"))?;
    if (stats.valid_measure - 4.0 * core::f64::consts::PI).abs() > 1.0e-12 {
        return Err(ConformanceError::Assertion("cb1 solid-angle weights do not sum to four pi"));
    }
    let center_key =
        DirCube::key(0, 1, 1, 2).map_err(|_| ConformanceError::Assertion("cb1 plan cell"))?;
    let center_query = veyra_core::sample::SampleQuery {
        field: scalar_field,
        pos: veyra_core::sample::Position::Cell { domain: "surface".to_owned(), key: center_key },
        level: veyra_core::sample::LevelSel::Exact(2),
        time: veyra_core::sample::TimeSel::Static,
    };
    if !cb1
        .plan(&center_query)
        .map_err(|_| ConformanceError::Assertion("cb1 pure sample plan"))?
        .is_empty()
    {
        return Err(ConformanceError::Assertion("fully loaded CB1 query still needs resources"));
    }
    verify_cb1_halos(&cb1, scalar_field)?;
    let constant_sample = cb1
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x7ffe, 2),
            pos: veyra_core::sample::Position::Cell {
                domain: "surface".to_owned(),
                key: DirCube::key(5, 0, 0, 2).unwrap(),
            },
            level: veyra_core::sample::LevelSel::Exact(2),
            time: veyra_core::sample::TimeSel::Static,
        })
        .map_err(|_| ConformanceError::Assertion("cb1 constant sample"))?;
    if constant_sample.source != veyra_core::sample::SampleSource::Const
        || constant_sample.category != Some(2)
    {
        return Err(ConformanceError::Assertion("cb1 constant tile source reporting"));
    }
    let inherited = cb1
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x7ffe, 2),
            pos: veyra_core::sample::Position::Cell {
                domain: "surface".to_owned(),
                key: DirCube::key(0, 0, 1, 2).unwrap(),
            },
            level: veyra_core::sample::LevelSel::Exact(3),
            time: veyra_core::sample::TimeSel::Static,
        })
        .map_err(|_| ConformanceError::Assertion("cb1 inherited sample"))?;
    if inherited.source != veyra_core::sample::SampleSource::Inherited || inherited.level_used != 2
    {
        return Err(ConformanceError::Assertion("cb1 inherited level/source reporting"));
    }
    let smoothed = cb1
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x7ffe, 3),
            pos: veyra_core::sample::Position::Cell {
                domain: "surface".to_owned(),
                key: DirCube::key(0, 1, 1, 2).unwrap(),
            },
            level: veyra_core::sample::LevelSel::Exact(3),
            time: veyra_core::sample::TimeSel::Slice(0),
        })
        .map_err(|_| ConformanceError::Assertion("cb1 smooth-only sample"))?;
    if smoothed.source != veyra_core::sample::SampleSource::Stored || smoothed.level_used != 2 {
        return Err(ConformanceError::Assertion("cb1 smooth-only level/source reporting"));
    }
    let cb2 =
        open_directory(world_root.join("cb2-categories")).map_err(ConformanceError::Writer)?;
    if !cb2.fields().iter().any(|field| field.semantic == "category") {
        return Err(ConformanceError::Assertion("cb2 category field is missing"));
    }
    let nodata_sample = cb2
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x0105, 1),
            pos: veyra_core::sample::Position::Cell {
                domain: "surface".to_owned(),
                key: DirCube::key(0, 0, 0, 2).unwrap(),
            },
            level: veyra_core::sample::LevelSel::Exact(2),
            time: veyra_core::sample::TimeSel::Static,
        })
        .map_err(|_| ConformanceError::Assertion("cb2 nodata sample"))?;
    if nodata_sample.source != veyra_core::sample::SampleSource::Nodata
        || nodata_sample.value.is_some()
    {
        return Err(ConformanceError::Assertion("cb2 nodata source reporting"));
    }
    let pyramid_sample = cb2
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x0105, 1),
            pos: veyra_core::sample::Position::Cell {
                domain: "surface".to_owned(),
                key: DirCube::key(0, 0, 0, 2).unwrap(),
            },
            level: veyra_core::sample::LevelSel::Exact(1),
            time: veyra_core::sample::TimeSel::Static,
        })
        .map_err(|_| ConformanceError::Assertion("cb2 stored pyramid sample"))?;
    if pyramid_sample.source != veyra_core::sample::SampleSource::Pyramid
        || pyramid_sample.category != Some(2)
    {
        return Err(ConformanceError::Assertion("cb2 below-native pyramid or tie handling"));
    }
    let category_histogram = cb2
        .histogram(
            FieldId::new(0x0105, 1),
            4,
            veyra_core::sample::LevelSel::Exact(2),
            veyra_core::sample::TimeSel::Static,
        )
        .map_err(|_| ConformanceError::Assertion("cb2 weighted histogram"))?;
    let histogram_weight: f64 = category_histogram.bins.iter().map(|bin| bin.weight).sum();
    let histogram_cells: u64 = category_histogram.bins.iter().map(|bin| bin.cells).sum();
    let mut expected_histogram_weight = 0.0;
    let mut expected_histogram_cells = 0;
    for face in 0..6_u8 {
        for i in 0..4_u64 {
            for j in 0..4_u64 {
                if (face, i, j) == (0, 0, 0) {
                    continue;
                }
                let key = DirCube::key(face, i, j, 2).unwrap();
                expected_histogram_weight += DirCube.cell_measure(key).unwrap();
                expected_histogram_cells += 1;
            }
        }
    }
    if histogram_cells != expected_histogram_cells
        || (histogram_weight - expected_histogram_weight).abs() > 1.0e-12
    {
        return Err(ConformanceError::Assertion(
            "cb2 histogram weights do not match valid cell measures",
        ));
    }
    let sphere_tile = DirCube.tile_key(DirCube::key(0, 0, 0, 2).unwrap(), 2).unwrap();
    let sphere_geometry = cb2
        .domain_geometry("surface", sphere_tile, 2)
        .map_err(|_| ConformanceError::Assertion("cb2 sphere geometry"))?;
    if sphere_geometry.vertices_m.iter().any(|point| {
        let radius_squared = point[0] * point[0] + point[1] * point[1] + point[2] * point[2];
        (radius_squared - 1.0).abs() > 1.0e-12
    }) {
        return Err(ConformanceError::Assertion(
            "cb2 sphere geometry does not preserve its declared radius",
        ));
    }
    let cb7 = open_directory(world_root.join("cb7-star1d")).map_err(ConformanceError::Writer)?;
    if cb7.figure().kind != "radial_profile_sphere"
        || cb7.capabilities().iter().any(|capability| {
            matches!(
                capability.id.as_str(),
                "veyra.cap.solid_surface/1"
                    | "veyra.cap.topography/1"
                    | "veyra.cap.tectonics/1"
                    | "veyra.cap.ocean/1"
                    | "veyra.cap.climate/1"
            )
        })
        || cb7.domains().len() != 1
        || cb7.domains()[0].topology != "veyra.topo.radial_1d/1"
        || cb7
            .reference_surfaces()
            .iter()
            .any(|surface| surface.get("kind").and_then(Value::as_str) == Some("figure_surface"))
    {
        return Err(ConformanceError::Assertion(
            "cb7 must be a radial stellar body without a solid surface",
        ));
    }
    let stellar_sample = cb7
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x0130, 1),
            pos: veyra_core::sample::Position::Radial { r_m: 5.0 },
            level: veyra_core::sample::LevelSel::Native,
            time: veyra_core::sample::TimeSel::Static,
        })
        .map_err(|_| ConformanceError::Assertion("cb7 radial interior sample"))?;
    if stellar_sample.value != Some(107.5) {
        return Err(ConformanceError::Assertion(
            "cb7 radial sample does not match its analytic profile",
        ));
    }
    let stellar_pyramid = cb7
        .sample(&veyra_core::sample::SampleQuery {
            field: FieldId::new(0x0130, 1),
            pos: veyra_core::sample::Position::Radial { r_m: 5.0 },
            level: veyra_core::sample::LevelSel::Exact(2),
            time: veyra_core::sample::TimeSel::Static,
        })
        .map_err(|_| ConformanceError::Assertion("cb7 radial pyramid sample"))?;
    if stellar_pyramid.source != veyra_core::sample::SampleSource::Pyramid
        || stellar_pyramid.level_used != 2
        || stellar_pyramid.value.is_none_or(|value| (value - 107.5).abs() > 1.0e-12)
    {
        return Err(ConformanceError::Assertion("cb7 radial pyramid source or value"));
    }
    let radial_stats = cb7
        .stats(
            FieldId::new(0x0130, 1),
            veyra_core::sample::LevelSel::Native,
            veyra_core::sample::TimeSel::Static,
        )
        .map_err(|_| ConformanceError::Assertion("cb7 volume-weighted statistics"))?;
    let sphere_volume = 4.0 * core::f64::consts::PI / 3.0 * 10.0 * 10.0 * 10.0;
    if (radial_stats.valid_measure - sphere_volume).abs() > sphere_volume * 1.0e-12 {
        return Err(ConformanceError::Assertion(
            "cb7 shell weights do not sum to the declared volume",
        ));
    }
    verify_cb7_halo(&cb7)?;
    if !cb7.views().iter().any(|view| view.group == "Stellar structure")
        || cb7.views().iter().any(|view| {
            matches!(view.group.as_str(), "Topography" | "Tectonics" | "Ocean" | "Climate")
        })
    {
        return Err(ConformanceError::Assertion("cb7 view catalog is not capability driven"));
    }
    let cb8 = open_directory(world_root.join("cb8-rock")).map_err(ConformanceError::Writer)?;
    if cb8.figure().kind != "star_convex_radial"
        || cb8.capabilities().iter().any(|capability| {
            matches!(
                capability.id.as_str(),
                "veyra.cap.topography/1"
                    | "veyra.cap.tectonics/1"
                    | "veyra.cap.ocean/1"
                    | "veyra.cap.climate/1"
            )
        })
        || cb8.root_value()["reference_surfaces"]
            .as_array()
            .is_none_or(|surfaces| surfaces.len() != 1)
        || cb8
            .section("dynamics/descriptor.json")
            .is_none_or(|descriptor| descriptor.get("orbit").is_some())
    {
        return Err(ConformanceError::Assertion(
            "cb8 must remain a non-spherical body without ocean or climate",
        ));
    }
    let rock_tile = DirCube.tile_key(DirCube::key(0, 0, 0, 2).unwrap(), 2).unwrap();
    let geometry = cb8
        .domain_geometry("surface", rock_tile, 4)
        .map_err(|_| ConformanceError::Assertion("cb8 radius-field geometry"))?;
    let radius_squared: Vec<f64> = geometry
        .vertices_m
        .iter()
        .map(|point| point[0] * point[0] + point[1] * point[1] + point[2] * point[2])
        .collect();
    let minimum = radius_squared.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let maximum = radius_squared.iter().copied().reduce(f64::max).unwrap_or(0.0);
    if maximum <= minimum {
        return Err(ConformanceError::Assertion(
            "cb8 geometry must use a non-spherical radius field",
        ));
    }
    let inspection = cb8
        .inspect(
            &veyra_core::sample::Position::Direction(
                veyra_core::spatial::Dir::new(1.0, 0.0, 0.0).unwrap(),
            ),
            veyra_core::sample::LevelSel::Native,
            veyra_core::sample::TimeSel::Static,
        )
        .map_err(|_| ConformanceError::Assertion("cb8 point inspection"))?;
    if inspection.fields.len() != 3
        || !inspection.fields.iter().any(|field| {
            field.name == "figure.radius_m"
                && field.sample.as_ref().is_some_and(|sample| sample.value.is_some())
        })
    {
        return Err(ConformanceError::Assertion("cb8 inspection did not report its stored fields"));
    }
    let cb9 =
        open_directory(world_root.join("cb9-minimal-void")).map_err(ConformanceError::Writer)?;
    if cb9
        .views()
        .iter()
        .any(|view| matches!(view.group.as_str(), "Topography" | "Tectonics" | "Ocean" | "Climate"))
    {
        return Err(ConformanceError::Assertion("cb9 must not declare terrestrial view groups"));
    }
    if !cb9.views().is_empty() {
        return Err(ConformanceError::Assertion("cb9 zero-capability body must not have views"));
    }
    let corpus_root =
        world_root.parent().ok_or(ConformanceError::Assertion("conformance world root parent"))?;
    let actual = run_stage4_queries(world_root, &corpus_root.join("queries/stage4.jsonl"))?;
    let expected = read_lines(&corpus_root.join("expected/stage4.jsonl"))?;
    if actual != expected {
        return Err(ConformanceError::Assertion(
            "Stage 4 query output differs from committed expected JSONL",
        ));
    }
    Ok(vec![
        FixtureResult {
            name: "cb1-seams".to_owned(),
            evidence: format!(
                "analytic cells and {edge_samples} face-edge samples PASS; weighted measure=4pi"
            ),
        },
        FixtureResult {
            name: "cb2-categories".to_owned(),
            evidence: "categorical and nodata raster verified".to_owned(),
        },
        FixtureResult {
            name: "cb7-star1d".to_owned(),
            evidence: format!(
                "radial interior sample={} with no solid surface",
                stellar_sample.value.unwrap()
            ),
        },
        FixtureResult {
            name: "cb8-rock".to_owned(),
            evidence: format!(
                "radius-field geometry varies from {:.0} to {:.0} m²",
                minimum, maximum
            ),
        },
        FixtureResult {
            name: "cb9-minimal-void".to_owned(),
            evidence: "zero-field view catalog remains capability-free".to_owned(),
        },
    ])
}

fn verify_cb1_halos(body: &veyra_core::io::Body, field: FieldId) -> Result<(), ConformanceError> {
    use veyra_core::spatial::FaceEdge;
    let edges = [FaceEdge::UMinus, FaceEdge::UPlus, FaceEdge::VMinus, FaceEdge::VPlus];
    for face in 0..6_u8 {
        let tile_key = DirCube.tile_key(DirCube::key(face, 0, 0, 2).unwrap(), 2).unwrap();
        let tile = body
            .tile(&veyra_core::sample::TileRequest {
                field,
                key: tile_key,
                time: veyra_core::sample::TimeSel::Static,
                halo: 1,
                view: veyra_core::sample::TileView::Raw,
            })
            .map_err(|_| ConformanceError::Assertion("cb1 halo tile"))?;
        if (tile.dim_i, tile.dim_j) != (6, 6) || tile.values.len() != 36 {
            return Err(ConformanceError::Assertion("cb1 halo dimensions"));
        }
        for edge in edges {
            for along in 0..4_u64 {
                let (source, target_i, target_j) = match edge {
                    FaceEdge::UMinus => {
                        (DirCube::key(face, 0, along, 2).unwrap(), 0_usize, along as usize + 1)
                    }
                    FaceEdge::UPlus => {
                        (DirCube::key(face, 3, along, 2).unwrap(), 5_usize, along as usize + 1)
                    }
                    FaceEdge::VMinus => {
                        (DirCube::key(face, along, 0, 2).unwrap(), along as usize + 1, 0_usize)
                    }
                    FaceEdge::VPlus => {
                        (DirCube::key(face, along, 3, 2).unwrap(), along as usize + 1, 5_usize)
                    }
                };
                let adjacent = DirCube.neighbor(source, edge).unwrap();
                let expected = sample_cell_value(body, field, "surface", adjacent, 2)?;
                let actual = tile.values[target_j * usize::from(tile.dim_i) + target_i]
                    .ok_or(ConformanceError::Assertion("cb1 halo cell is nodata"))?;
                if (actual - expected).abs() > 1.0e-6 {
                    return Err(ConformanceError::Assertion("cb1 cross-face halo value"));
                }
            }
        }
        for (i, j, output_i, output_j) in
            [(0, 0, 0_usize, 0_usize), (3, 0, 5, 0), (0, 3, 0, 5), (3, 3, 5, 5)]
        {
            let corner = DirCube::key(face, i, j, 2).unwrap();
            let adjacent = DirCube.corner_stencil(corner).unwrap().unwrap();
            let left = sample_cell_value(body, field, "surface", adjacent[0], 2)?;
            let right = sample_cell_value(body, field, "surface", adjacent[1], 2)?;
            let expected = (left + right) / 2.0;
            let actual = tile.values[output_j * usize::from(tile.dim_i) + output_i]
                .ok_or(ConformanceError::Assertion("cb1 corner halo is nodata"))?;
            if (actual - expected).abs() > 1.0e-6 {
                return Err(ConformanceError::Assertion("cb1 cube-corner halo rule"));
            }
        }
    }
    Ok(())
}

fn verify_cb7_halo(body: &veyra_core::io::Body) -> Result<(), ConformanceError> {
    let field = FieldId::new(0x0130, 1);
    let first = Radial1d::default().tile_key(Radial1d::key(4, 0).unwrap(), 2).unwrap();
    let first_tile = body
        .tile(&veyra_core::sample::TileRequest {
            field,
            key: first,
            time: veyra_core::sample::TimeSel::Static,
            halo: 1,
            view: veyra_core::sample::TileView::Raw,
        })
        .map_err(|_| ConformanceError::Assertion("cb7 radial halo tile"))?;
    let start = sample_cell_value(body, field, "interior", Radial1d::key(4, 0).unwrap(), 4)?;
    let next_tile = sample_cell_value(body, field, "interior", Radial1d::key(4, 4).unwrap(), 4)?;
    if first_tile.dim_i != 6
        || first_tile.values[0] != Some(start)
        || first_tile.values[1] != Some(start)
        || first_tile.values[5] != Some(next_tile)
    {
        return Err(ConformanceError::Assertion("cb7 radial halo clamping or neighbor"));
    }
    let last = Radial1d::default().tile_key(Radial1d::key(4, 12).unwrap(), 2).unwrap();
    let last_tile = body
        .tile(&veyra_core::sample::TileRequest {
            field,
            key: last,
            time: veyra_core::sample::TimeSel::Static,
            halo: 1,
            view: veyra_core::sample::TileView::Raw,
        })
        .map_err(|_| ConformanceError::Assertion("cb7 final radial halo tile"))?;
    let end = sample_cell_value(body, field, "interior", Radial1d::key(4, 15).unwrap(), 4)?;
    if last_tile.values[4] != Some(end) || last_tile.values[5] != Some(end) {
        return Err(ConformanceError::Assertion("cb7 radial endpoint halo clamp"));
    }
    Ok(())
}

fn sample_cell_value(
    body: &veyra_core::io::Body,
    field: FieldId,
    domain: &str,
    key: CellKey,
    level: u8,
) -> Result<f64, ConformanceError> {
    body.sample(&veyra_core::sample::SampleQuery {
        field,
        pos: veyra_core::sample::Position::Cell { domain: domain.to_owned(), key },
        level: veyra_core::sample::LevelSel::Exact(level),
        time: veyra_core::sample::TimeSel::Static,
    })
    .map_err(|_| ConformanceError::Assertion("sample addressed conformance cell"))?
    .value
    .ok_or(ConformanceError::Assertion("sampled conformance cell is nodata"))
}

fn run_stage4_queries(
    world_root: &Path,
    query_path: &Path,
) -> Result<Vec<String>, ConformanceError> {
    let mut bodies = std::collections::BTreeMap::new();
    let mut output = Vec::new();
    for query in read_lines(query_path)? {
        let value: Value = serde_json::from_str(&query)
            .map_err(|_| ConformanceError::Assertion("Stage 4 query JSON"))?;
        let fixture =
            value["fixture"].as_str().ok_or(ConformanceError::Assertion("Stage 4 fixture name"))?;
        if !bodies.contains_key(fixture) {
            bodies.insert(
                fixture.to_owned(),
                open_directory(world_root.join(fixture)).map_err(ConformanceError::Writer)?,
            );
        }
        let body = bodies.get(fixture).ok_or(ConformanceError::Assertion("Stage 4 body cache"))?;
        let operation =
            value["op"].as_str().ok_or(ConformanceError::Assertion("Stage 4 query operation"))?;
        let result = match operation {
            "sample" => {
                let field_text =
                    value["field"].as_str().ok_or(ConformanceError::Assertion("Stage 4 field"))?;
                let field = FieldId::parse(field_text)
                    .map_err(|_| ConformanceError::Assertion("Stage 4 field ID"))?;
                let position = query_position(&value["position"])?;
                let level = query_level(&value["level"])?;
                let time = query_time(&value["time"])?;
                let sample = body
                    .sample(&veyra_core::sample::SampleQuery { field, pos: position, level, time })
                    .map_err(|_| ConformanceError::Assertion("Stage 4 sample query"))?;
                json!({"value_bits":sample.value.map(f64_bits),"raw":sample.raw.map(raw_json),"category":sample.category,"level_used":sample.level_used,"source":format!("{:?}",sample.source),"cell":format_key(sample.cell.0)})
            }
            "stats" => {
                let field = query_field(&value["field"])?;
                let stats = body
                    .stats(field, query_level(&value["level"])?, query_time(&value["time"])?)
                    .map_err(|_| ConformanceError::Assertion("Stage 4 stats query"))?;
                json!({"cells":stats.cells,"nodata_cells":stats.nodata_cells,"valid_measure_bits":f64_bits(stats.valid_measure),"minimum_bits":stats.minimum.map(f64_bits),"maximum_bits":stats.maximum.map(f64_bits),"mean_bits":stats.mean.map(f64_bits)})
            }
            "histogram" => {
                let bins = value["bins"]
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(ConformanceError::Assertion("Stage 4 histogram bins"))?;
                let histogram = body
                    .histogram(
                        query_field(&value["field"])?,
                        bins,
                        query_level(&value["level"])?,
                        query_time(&value["time"])?,
                    )
                    .map_err(|_| ConformanceError::Assertion("Stage 4 histogram query"))?;
                json!({"minimum_bits":histogram.minimum.map(f64_bits),"maximum_bits":histogram.maximum.map(f64_bits),"bins":histogram.bins.iter().map(|bin| json!({"lower_bits":f64_bits(bin.lower),"upper_bits":f64_bits(bin.upper),"weight_bits":f64_bits(bin.weight),"cells":bin.cells})).collect::<Vec<_>>()})
            }
            "tile" => {
                let key_value = &value["key"];
                let key = veyra_core::spatial::TileKey {
                    level: u8::try_from(
                        key_value["level"]
                            .as_u64()
                            .ok_or(ConformanceError::Assertion("tile query level"))?,
                    )
                    .map_err(|_| ConformanceError::Assertion("tile query level range"))?,
                    address: CellKey(parse_key(
                        key_value["address"]
                            .as_str()
                            .ok_or(ConformanceError::Assertion("tile query address"))?,
                    )?),
                };
                let view = match value["view"].as_str() {
                    Some("raw") => veyra_core::sample::TileView::Raw,
                    Some("time_reduce") => veyra_core::sample::TileView::TimeReduce,
                    Some(view) if view.starts_with("derived:") => {
                        veyra_core::sample::TileView::Derived(view[8..].to_owned())
                    }
                    _ => return Err(ConformanceError::Assertion("tile query view")),
                };
                let tile = body
                    .tile(&veyra_core::sample::TileRequest {
                        field: query_field(&value["field"])?,
                        key,
                        time: query_time(&value["time"])?,
                        halo: u8::try_from(
                            value["halo"]
                                .as_u64()
                                .ok_or(ConformanceError::Assertion("tile query halo"))?,
                        )
                        .map_err(|_| ConformanceError::Assertion("tile query halo range"))?,
                        view,
                    })
                    .map_err(|_| ConformanceError::Assertion("Stage 4 tile query"))?;
                json!({"dim_i":tile.dim_i,"dim_j":tile.dim_j,"slices":tile.slices,"source":format!("{:?}",tile.source),"values_bits":tile.values.iter().map(|value| value.map(f64_bits)).collect::<Vec<_>>()})
            }
            "views" => {
                json!({"views":body.views().iter().map(|view| json!({"id":view.id,"group":view.group,"label":view.label,"field_name":view.field_name})).collect::<Vec<_>>() })
            }
            "geometry" => {
                let domain = value["domain"]
                    .as_str()
                    .ok_or(ConformanceError::Assertion("Stage 4 geometry domain"))?;
                let tile_value = &value["tile"];
                let tile = veyra_core::spatial::TileKey {
                    level: u8::try_from(
                        tile_value["level"]
                            .as_u64()
                            .ok_or(ConformanceError::Assertion("geometry level"))?,
                    )
                    .map_err(|_| ConformanceError::Assertion("geometry level range"))?,
                    address: CellKey(parse_key(
                        tile_value["address"]
                            .as_str()
                            .ok_or(ConformanceError::Assertion("geometry key"))?,
                    )?),
                };
                let grid_n = u16::try_from(
                    value["grid_n"].as_u64().ok_or(ConformanceError::Assertion("geometry grid"))?,
                )
                .map_err(|_| ConformanceError::Assertion("geometry grid range"))?;
                let geometry = body
                    .domain_geometry(domain, tile, grid_n)
                    .map_err(|_| ConformanceError::Assertion("Stage 4 geometry query"))?;
                let radius_squared: Vec<f64> = geometry
                    .vertices_m
                    .iter()
                    .map(|point| point[0] * point[0] + point[1] * point[1] + point[2] * point[2])
                    .collect();
                json!({"vertices":geometry.vertices_m.len(),"triangles":geometry.triangles.len(),"min_radius_squared_bits":radius_squared.iter().copied().reduce(f64::min).map(f64_bits),"max_radius_squared_bits":radius_squared.iter().copied().reduce(f64::max).map(f64_bits)})
            }
            _ => return Err(ConformanceError::Assertion("unknown Stage 4 query operation")),
        };
        output.push(
            serde_json::to_string(&json!({"fixture":fixture,"op":operation,"result":result}))
                .map_err(|_| ConformanceError::Assertion("Stage 4 query output JSON"))?,
        );
    }
    Ok(output)
}

fn query_field(value: &Value) -> Result<FieldId, ConformanceError> {
    FieldId::parse(value.as_str().ok_or(ConformanceError::Assertion("Stage 4 field ID"))?)
        .map_err(|_| ConformanceError::Assertion("Stage 4 field ID"))
}

fn query_position(value: &Value) -> Result<veyra_core::sample::Position, ConformanceError> {
    match value["kind"].as_str() {
        Some("direction") => {
            let xyz =
                value["xyz"].as_array().ok_or(ConformanceError::Assertion("query direction"))?;
            if xyz.len() != 3 {
                return Err(ConformanceError::Assertion("query direction arity"));
            }
            let components = xyz
                .iter()
                .map(|value| {
                    value.as_f64().ok_or(ConformanceError::Assertion("query direction component"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let direction =
                veyra_core::spatial::Dir::new(components[0], components[1], components[2])
                    .map_err(|_| ConformanceError::Assertion("query direction"))?;
            Ok(veyra_core::sample::Position::Direction(direction))
        }
        Some("cell") => Ok(veyra_core::sample::Position::Cell {
            domain: value["domain"]
                .as_str()
                .ok_or(ConformanceError::Assertion("query cell domain"))?
                .to_owned(),
            key: CellKey(parse_key(
                value["key"].as_str().ok_or(ConformanceError::Assertion("query cell key"))?,
            )?),
        }),
        Some("radius") => Ok(veyra_core::sample::Position::Radial {
            r_m: value["r_m"].as_f64().ok_or(ConformanceError::Assertion("query radius"))?,
        }),
        _ => Err(ConformanceError::Assertion("unknown Stage 4 position kind")),
    }
}

fn query_level(value: &Value) -> Result<veyra_core::sample::LevelSel, ConformanceError> {
    if let Some(level) = value.as_u64() {
        return u8::try_from(level)
            .map(veyra_core::sample::LevelSel::Exact)
            .map_err(|_| ConformanceError::Assertion("query level range"));
    }
    match value.as_str() {
        Some("native") => Ok(veyra_core::sample::LevelSel::Native),
        Some("canonical") => Ok(veyra_core::sample::LevelSel::Canonical),
        _ => Err(ConformanceError::Assertion("unknown Stage 4 level")),
    }
}

fn query_time(value: &Value) -> Result<veyra_core::sample::TimeSel, ConformanceError> {
    let Some(value) = value.as_str() else {
        return Err(ConformanceError::Assertion("Stage 4 time selector"));
    };
    match value {
        "static" => Ok(veyra_core::sample::TimeSel::Static),
        "mean" => Ok(veyra_core::sample::TimeSel::Mean),
        "min" => Ok(veyra_core::sample::TimeSel::Min),
        "max" => Ok(veyra_core::sample::TimeSel::Max),
        _ if value.starts_with("slice:") => value[6..]
            .parse::<u16>()
            .map(veyra_core::sample::TimeSel::Slice)
            .map_err(|_| ConformanceError::Assertion("Stage 4 slice number")),
        _ if value.starts_with("phase:") => value[6..]
            .parse::<f64>()
            .map(veyra_core::sample::TimeSel::Phase)
            .map_err(|_| ConformanceError::Assertion("Stage 4 phase number")),
        _ => Err(ConformanceError::Assertion("unknown Stage 4 time selector")),
    }
}

fn raw_json(raw: veyra_core::sample::RawValue) -> Value {
    match raw {
        veyra_core::sample::RawValue::Integer(value) => json!(value),
        veyra_core::sample::RawValue::Float(value) => json!(format!("0x{:08x}", value.to_bits())),
    }
}

fn f64_bits(value: f64) -> String {
    format!("0x{:016x}", value.to_bits())
}

fn read_lines(path: &Path) -> Result<Vec<String>, ConformanceError> {
    let text = fs::read_to_string(path).map_err(ConformanceError::Io)?;
    Ok(text.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
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
        vec![json!({"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}})],
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
        "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed","rotation":{"kind":"uniform","period_s":"86400","epoch":"0","orientation_q_at_epoch":["1","0","0","0"],"relative_to":"universe_inertial"}}},
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
        "veyra.cap.solid_surface/1",
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
    let id = match capability {
        "veyra.cap.topography/1" => FieldId::new(0x0101, 1),
        "veyra.cap.solid_surface/1" => FieldId::new(0x0100, 1),
        _ => FIELD_ID,
    };
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
