use std::path::Path;

#[test]
fn committed_foundation_and_stage4_fixtures_match_golden_queries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let results = veyra_conformance::verify_foundation(
        root.join("conformance/worlds"),
        root.join("conformance/vectors"),
    )
    .unwrap();
    assert_eq!(results.len(), 9);
}
