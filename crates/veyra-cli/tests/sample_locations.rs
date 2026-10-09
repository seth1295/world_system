use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn cli_sample_and_inspect_report_the_query_containing_bilinear_cell() {
    let body = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/worlds/cb1-seams");
    let field_id = "0x7ffe0001";
    let expected_cell = "0x1d00000000000000";

    let sample = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "sample"])
        .arg(&body)
        .args(["--field", field_id, "--dir", "1,1,0", "--level", "2", "--time", "static"])
        .output()
        .expect("run sample command");
    assert!(sample.status.success(), "{}", String::from_utf8_lossy(&sample.stderr));
    let sample_json: Value = serde_json::from_slice(&sample.stdout).expect("sample JSON");
    assert_eq!(sample_json["cell"], expected_cell);

    let inspect = Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "inspect"])
        .arg(&body)
        .args(["--dir", "1,1,0", "--level", "2", "--time", "static"])
        .output()
        .expect("run inspect command");
    assert!(inspect.status.success(), "{}", String::from_utf8_lossy(&inspect.stderr));
    let inspect_json: Value = serde_json::from_slice(&inspect.stdout).expect("inspect JSON");
    let inspected_sample = inspect_json["fields"]
        .as_array()
        .and_then(|fields| fields.iter().find(|field| field["id"] == field_id))
        .expect("sampled field appears in inspect output");
    assert_eq!(inspected_sample["sample"]["cell"], expected_cell);
    assert_eq!(inspected_sample["sample"]["cell"], sample_json["cell"]);
}
