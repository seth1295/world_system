//! Feature-gated deterministic payload used only for executable foundation parity checks.

use veyra_core::canon::blob::{BlobKind, CanonicalBlob, DType};
use veyra_core::canon::hash::{self, body_seed, detail_hash, mix64, subseed};
use veyra_core::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
use veyra_core::canon::{jcs, ledger};
use veyra_core::ids::{ObjectAddress, ObjectId, RegionKey, UniverseId};
use veyra_core::spatial::{Dir, DirCube, Radial1d};
use veyra_core::time::UTime;

const RFC_VECTORS: &str =
    include_str!("../../../conformance/vectors/canon/rfc8785-appendix-b.json");
const STAGE2_VECTORS: &str =
    include_str!("../../../conformance/vectors/topology/stage2-parity.json");

/// Returns Stage 1 canonical vectors followed by 100,000 deterministic Stage 2 queries.
pub fn foundation_parity_payload() -> Result<Vec<u8>, String> {
    let sample_count = foundation_parity_sample_count()?;
    let mut random_state = parse_hex_u64(json_string_field(STAGE2_VECTORS, "seed")?)?;

    let mut output = b"VEYRA-FOUNDATION-PARITY\0\x01".to_vec();
    append_segment(&mut output, b"JCS-STRICT\0");
    let jcs_input = br#"{"z":-2,"a":"value"}"#;
    let jcs_bytes = jcs::canonicalize_json(jcs_input).map_err(|error| error.to_string())?;
    append_segment(&mut output, &jcs_bytes);
    output.extend_from_slice(&hash::hash(&jcs_bytes).0);

    append_segment(&mut output, b"JCS-RFC8785-APPENDIX-B\0");
    for row in RFC_VECTORS.lines().filter(|line| line.trim_start().starts_with("{\"name\"")) {
        let bits = parse_hex_u64(json_string_field(row, "bits")?)?;
        let number = f64::from_bits(bits);
        output.extend_from_slice(&bits.to_le_bytes());
        match jcs::serialize_rfc8785_number(number) {
            Ok(canonical) => {
                if json_string_field(row, "canonical")? != canonical {
                    return Err("RFC 8785 vector mismatch in parity payload".to_owned());
                }
                output.push(1);
                append_segment(&mut output, canonical.as_bytes());
            }
            Err(_) if row.contains("\"canonical\":null") => output.push(0),
            Err(_) => return Err("unexpected RFC 8785 number rejection".to_owned()),
        }
    }

    let blob = CanonicalBlob::new(BlobKind::RasterTile, DType::I16, 2, 1, 1, vec![1, 0, 2, 0])
        .map_err(|error| format!("parity blob construction failed: {error}"))?
        .encode();
    append_segment(&mut output, b"VYB1\0");
    append_segment(&mut output, &blob);
    output.extend_from_slice(&hash::hash(&blob).0);

    let index = IndexBlob {
        field_id: 0x0101_0001,
        topology: TopologyTag::DirCube,
        tile_log2: 3,
        entries: vec![IndexEntry { level: 0, key: 1_u64 << 60, value: IndexValue::Const(-7) }],
    }
    .encode()
    .map_err(|error| format!("parity index construction failed: {error}"))?;
    append_segment(&mut output, b"INDEX\0");
    append_segment(&mut output, &index);
    output.extend_from_slice(&hash::hash(&index).0);

    let universe = UniverseId([7; 32]);
    let fixture_address = ObjectAddress::Fixture { name: "cb0-addressing".to_owned() };
    let object_id =
        ObjectId::derive(universe, &fixture_address).map_err(|error| error.to_string())?;
    append_segment(&mut output, b"IDENTITY\0");
    output.extend_from_slice(&universe.0);
    output.extend_from_slice(&fixture_address.encode().map_err(|error| error.to_string())?);
    output.extend_from_slice(&object_id.0);

    let addresses = [
        ObjectAddress::SystemSeed {
            region: RegionKey { level: 0, ix: -1, iy: 64, iz: 0 },
            slot: 300,
        },
        ObjectAddress::BodyInSystem { system: ObjectId([0x11; 16]), role: 2, ordinal: 128 },
        ObjectAddress::FreeObject {
            region: RegionKey { level: 0, ix: -2, iy: 3, iz: -4 },
            slot: 1,
        },
        fixture_address,
    ];
    for address in addresses {
        append_segment(&mut output, &address.encode().map_err(|error| error.to_string())?);
    }

    let seed_id = ObjectId([0x42; 16]);
    let seed = body_seed(seed_id);
    let detail_seed = subseed(seed, "refine/example");
    append_segment(&mut output, b"SEEDS\0");
    output.extend_from_slice(&seed.to_le_bytes());
    output.extend_from_slice(&detail_seed.to_le_bytes());
    output.extend_from_slice(&mix64(0).to_le_bytes());
    output.extend_from_slice(&detail_hash(detail_seed, 0x1234, 2).to_le_bytes());

    let golden_vectors = include_str!("../../../conformance/vectors/canon/golden.json");
    let ledger_bytes = decode_hex(json_string_field(golden_vectors, "bytes_hex")?)?;
    let ledger_expected_head = json_string_field(golden_vectors, "head")?;
    let ledger_head = ledger::verify(&ledger_bytes).map_err(|error| error.to_string())?;
    if ledger_head.hash.to_string() != ledger_expected_head {
        return Err("committed ledger vector mismatch in parity payload".to_owned());
    }
    append_segment(&mut output, b"LEDGER\0");
    append_segment(&mut output, &ledger_bytes);
    output.extend_from_slice(&ledger_head.count.to_le_bytes());
    output.extend_from_slice(&ledger_head.hash.0);

    let timestamp = UTime::from_nanos(-1_234_567_890);
    let timestamp_text = timestamp.decimal();
    if timestamp_text.parse::<UTime>().map_err(|error| error.to_string())? != timestamp {
        return Err("UTime parity round trip failed".to_owned());
    }
    append_segment(&mut output, b"UTIME\0");
    append_segment(&mut output, timestamp_text.as_bytes());

    append_segment(&mut output, b"TOPOLOGY-BOUNDARIES\0");
    append_fixed_topology_cases(&mut output)?;
    append_segment(&mut output, b"TOPOLOGY-RANDOM-100000\0");
    output.extend_from_slice(&sample_count.to_le_bytes());
    output.extend_from_slice(&random_state.to_le_bytes());
    for ordinal in 0..sample_count {
        let x = random_component(&mut random_state);
        let y = random_component(&mut random_state);
        let z = random_component(&mut random_state);
        let direction = Dir::new(x, y, z).map_err(|error| error.to_string())?;
        let level =
            u8::try_from(next_random(&mut random_state) % 31).map_err(|error| error.to_string())?;
        let cube_key = DirCube.locate(direction, level).map_err(|error| error.to_string())?;
        let cube_center = DirCube.cell_center(cube_key).map_err(|error| error.to_string())?;
        if DirCube.locate(cube_center, level).map_err(|error| error.to_string())? != cube_key {
            return Err(format!("cube round trip failed at parity case {ordinal}"));
        }
        let radial_count = 1_u64 << level;
        let radial_index = next_random(&mut random_state) % radial_count;
        let radial_key = Radial1d::key(level, radial_index).map_err(|error| error.to_string())?;
        let (decoded_level, decoded_index) =
            Radial1d::decode(radial_key).map_err(|error| error.to_string())?;
        if Radial1d::key(decoded_level, decoded_index).map_err(|error| error.to_string())?
            != radial_key
        {
            return Err(format!("radial round trip failed at parity case {ordinal}"));
        }
        output.push(level);
        output.extend_from_slice(&x.to_bits().to_le_bytes());
        output.extend_from_slice(&y.to_bits().to_le_bytes());
        output.extend_from_slice(&z.to_bits().to_le_bytes());
        output.extend_from_slice(&cube_key.0.to_le_bytes());
        output.extend_from_slice(&radial_key.0.to_le_bytes());
        output.extend_from_slice(
            &Radial1d::default()
                .center_radius(radial_key)
                .map_err(|error| error.to_string())?
                .to_bits()
                .to_le_bytes(),
        );
    }
    Ok(output)
}

fn append_fixed_topology_cases(output: &mut Vec<u8>) -> Result<(), String> {
    let cube = DirCube;
    let mut directions = Vec::new();
    for (x, y, z) in [
        (1.0, 0.0, 0.0),
        (-1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
        (0.0, -1.0, 0.0),
        (0.0, 0.0, 1.0),
        (0.0, 0.0, -1.0),
        (1.0, 1.0, 1.0),
        (1.0, 1.0, -1.0),
        (1.0, -1.0, 1.0),
        (1.0, -1.0, -1.0),
        (-1.0, 1.0, 1.0),
        (-1.0, 1.0, -1.0),
        (-1.0, -1.0, 1.0),
        (-1.0, -1.0, -1.0),
    ] {
        directions.push(Dir::new(x, y, z).map_err(|error| error.to_string())?);
    }
    for a in [-1.0, 1.0] {
        for b in [-1.0, 1.0] {
            for along in [-0.5, 0.0, 0.5] {
                directions.push(Dir::new(a, b, along).map_err(|error| error.to_string())?);
                directions.push(Dir::new(a, along, b).map_err(|error| error.to_string())?);
                directions.push(Dir::new(along, a, b).map_err(|error| error.to_string())?);
            }
        }
    }
    for direction in directions {
        for level in [0, 1, 5, 30] {
            let key = cube.locate(direction, level).map_err(|error| error.to_string())?;
            output.push(level);
            output.extend_from_slice(&key.0.to_le_bytes());
        }
    }
    for level in [0, 1, 5, 30] {
        let count = 1_u64 << level;
        for index in [0, count / 2, count - 1] {
            let key = Radial1d::key(level, index).map_err(|error| error.to_string())?;
            let (decoded_level, decoded_index) =
                Radial1d::decode(key).map_err(|error| error.to_string())?;
            if Radial1d::key(decoded_level, decoded_index).map_err(|error| error.to_string())?
                != key
            {
                return Err("fixed radial key round trip failed".to_owned());
            }
            output.push(level);
            output.extend_from_slice(&key.0.to_le_bytes());
            output.extend_from_slice(
                &Radial1d::default()
                    .center_radius(key)
                    .map_err(|error| error.to_string())?
                    .to_bits()
                    .to_le_bytes(),
            );
        }
    }
    Ok(())
}

fn next_random(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mix64(*state)
}

fn random_unit(state: &mut u64) -> f64 {
    const SCALE: f64 = 1.0 / 9_007_199_254_740_992.0;
    ((next_random(state) >> 11) as f64) * SCALE
}

fn random_component(state: &mut u64) -> f64 {
    random_unit(state) * 2.0 - 1.0
}

fn append_segment(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).expect("parity segment fits in u32");
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(value);
}

fn parse_hex_u64(text: &str) -> Result<u64, String> {
    u64::from_str_radix(text.strip_prefix("0x").unwrap_or(text), 16)
        .map_err(|error| error.to_string())
}

/// Returns the pinned Stage 2 case count from the committed vector descriptor.
pub fn foundation_parity_sample_count() -> Result<u64, String> {
    json_unsigned_field(STAGE2_VECTORS, "sample_count")
}

/// Returns the pinned BLAKE3 digest of the native/WASM parity payload.
pub fn foundation_parity_expected_hash() -> Result<String, String> {
    json_string_field(STAGE2_VECTORS, "expected_output_hash").map(str::to_owned)
}

fn json_string_field<'a>(document: &'a str, key: &str) -> Result<&'a str, String> {
    let marker = format!("\"{key}\"");
    let tail = document
        .split_once(&marker)
        .map(|(_, tail)| tail)
        .ok_or_else(|| format!("missing JSON string field {key}"))?;
    let tail = tail
        .split_once(':')
        .map(|(_, tail)| tail.trim_start())
        .ok_or_else(|| format!("malformed JSON string field {key}"))?;
    let tail = tail.strip_prefix('"').ok_or_else(|| format!("JSON field {key} is not a string"))?;
    let end = tail.find('"').ok_or_else(|| format!("unterminated JSON string field {key}"))?;
    Ok(&tail[..end])
}

fn json_unsigned_field(document: &str, key: &str) -> Result<u64, String> {
    let marker = format!("\"{key}\"");
    let tail = document
        .split_once(&marker)
        .map(|(_, tail)| tail)
        .ok_or_else(|| format!("missing JSON integer field {key}"))?;
    let tail = tail
        .split_once(':')
        .map(|(_, tail)| tail.trim_start())
        .ok_or_else(|| format!("malformed JSON integer field {key}"))?;
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().map_err(|error: std::num::ParseIntError| error.to_string())
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex vector has an odd number of digits".to_owned());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16).map_err(|error| error.to_string())
        })
        .collect()
}
