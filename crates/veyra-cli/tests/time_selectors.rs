use std::path::Path;
use std::process::{Command, Output};

fn run_sample(selectors: &[&str]) -> Output {
    let body = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/worlds/cb1-seams");
    Command::new(env!("CARGO_BIN_EXE_veyra"))
        .args(["body", "sample"])
        .arg(body)
        .args(["--field", "0x7ffe0001", "--cell", "surface:0x0700000000000000", "--level", "2"])
        .args(selectors)
        .output()
        .expect("run body sample")
}

#[test]
fn sample_command_rejects_conflicting_time_flags_in_either_order() {
    for selectors in [
        &["--slice", "1", "--time", "min"][..],
        &["--time", "min", "--slice", "1"][..],
        &["--slice", "1", "--time", "invalid"][..],
        &["--time", "invalid", "--slice", "1"][..],
        &["--slice", "invalid", "--time", "min"][..],
    ] {
        let output = run_sample(selectors);
        assert!(!output.status.success(), "selectors {selectors:?} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--slice and --time are mutually exclusive"),
            "selectors {selectors:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
