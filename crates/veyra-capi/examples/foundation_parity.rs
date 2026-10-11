use std::error::Error;
use std::fs;
use std::path::PathBuf;

use veyra_core::canon::hash;

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os().nth(1).map(PathBuf::from).ok_or("output path is required")?;
    let bytes = veyra_capi::foundation_parity_payload().map_err(std::io::Error::other)?;
    let expected = veyra_capi::foundation_parity_expected_hash().map_err(std::io::Error::other)?;
    let actual = hash::hash(&bytes).to_string();
    if expected != actual {
        return Err(format!(
            "foundation parity payload hash changed: expected {expected}, got {actual}"
        )
        .into());
    }
    fs::write(&output, &bytes)?;
    println!(
        "PASS native foundation payload bytes={} blake3={} random_cases={}",
        bytes.len(),
        actual,
        veyra_capi::foundation_parity_sample_count().map_err(std::io::Error::other)?
    );
    Ok(())
}
