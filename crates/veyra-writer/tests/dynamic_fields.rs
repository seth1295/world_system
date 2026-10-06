use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use veyra_core::body::ModelError;
use veyra_core::ids::{ObjectAddress, ObjectId, UniverseId};
use veyra_writer::{ArtifactWriter, WriterError};

#[test]
fn baseline_writer_refuses_dynamic_fields() {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir()
        .join(format!("veyra-dynamic-baseline-{}-{unique}", std::process::id()));
    let mut writer = ArtifactWriter::new(&path).unwrap();

    let registry = json!({
        "schema":"veyra.field_registry/1",
        "fields":[{
            "id":"0x7ffe0001",
            "name":"x-dynamic-test",
            "capability":"x-veyra.conformance.foundation/1",
            "domain":"",
            "semantic":"x-test/1",
            "persistence":"dynamic",
            "storage":{"dtype":"u8"},
            "native_level":0,
            "temporal":{"kind":"static"},
            "sampling":{"interp":"nearest"},
            "downsample":"mean",
            "compat":"ancillary"
        }]
    });
    let descriptor = json!({"schema":"veyra.dynamics_descriptor/1"});
    let origin = json!({"schema":"veyra.dynamics_origin/1"});
    let registry_hash = writer
        .write_json_section("registry/fields.json", &serde_json::to_vec(&registry).unwrap())
        .unwrap();
    let descriptor_hash = writer
        .write_json_section("dynamics/descriptor.json", &serde_json::to_vec(&descriptor).unwrap())
        .unwrap();
    let origin_hash = writer
        .write_json_section("dynamics/origin.json", &serde_json::to_vec(&origin).unwrap())
        .unwrap();
    let object_id = ObjectId::derive(
        UniverseId::fixture_sentinel(),
        &ObjectAddress::Fixture { name: "dynamic-test".to_owned() },
    )
    .unwrap();
    let body = json!({
        "schema":"veyra.body/1",
        "format_version":{"major":1,"minor":0},
        "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1"],
        "identity":{"object_id":object_id.to_string(),"origin":{"kind":"fixture","name":"dynamic-test"}},
        "classification":{},
        "physical":{"gm_m3_s2":"1"},
        "figure":{"kind":"sphere","radius_m":"1"},
        "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed","rotation":{"kind":"uniform","period_s":"86400","epoch":"0","orientation_q_at_epoch":["1","0","0","0"],"relative_to":"universe_inertial"}}},
        "reference_surfaces":[],
        "dynamics":{"descriptor":{"path":"dynamics/descriptor.json","hash":descriptor_hash.to_string()},"origin_keyframe":{"path":"dynamics/origin.json","hash":origin_hash.to_string()}},
        "capabilities":[{"id":"x-veyra.conformance.foundation/1","compat":"ancillary","params":{}}],
        "domains":[],
        "codec":"zstd+shuffle2",
        "sections":{"registry":{"path":"registry/fields.json","hash":registry_hash.to_string()}},
        "indexes":{}
    });

    let result = writer.write_body_json(&serde_json::to_vec(&body).unwrap());
    assert!(matches!(result, Err(WriterError::Model(ModelError::DynamicBaselineField))));
    drop(writer);
    fs::remove_dir_all(path).unwrap();
}
