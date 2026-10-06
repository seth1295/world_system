use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use veyra_core::canon::{hash, jcs};
use veyra_core::ids::{ObjectAddress, ObjectId, UniverseId};
use veyra_core::spatial::{CellKey, Dir, DirCube, Radial1d, Topology};
use veyra_writer::{open_directory, verify_directory};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    match command {
        "--help" | "-h" | "help" => print_help(),
        "fmt" => command_fmt(&args[1..])?,
        "hash" => command_hash(&args[1..], false)?,
        "hash-json" => command_hash(&args[1..], true)?,
        "id" => command_id(&args[1..])?,
        "cell" => command_cell(&args[1..])?,
        "tile-key" => command_tile_key(&args[1..])?,
        "body" => command_body(&args[1..])?,
        "conformance" => command_conformance(&args[1..])?,
        other => return Err(format!("unknown command {other}; use --help").into()),
    }
    Ok(())
}

fn command_fmt(args: &[String]) -> Result<(), Box<dyn Error>> {
    let mode = args.first().ok_or("usage: veyra fmt --canonical|--pretty <file>")?;
    let path = args.get(1).ok_or("missing JSON file path")?;
    let input = fs::read(path)?;
    let output = format_json(&input, mode)?;
    println!("{}", String::from_utf8(output)?);
    Ok(())
}

fn format_json(input: &[u8], mode: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    match mode {
        "--canonical" => Ok(jcs::canonicalize_json(input)?),
        "--pretty" => {
            let value = jcs::parse_unique_json(input)?;
            Ok(serde_json::to_vec_pretty(&value)?)
        }
        _ => Err("format mode must be --canonical or --pretty".into()),
    }
}

fn command_hash(args: &[String], canonical_json: bool) -> Result<(), Box<dyn Error>> {
    let path = args.first().ok_or("missing file path")?;
    let bytes = fs::read(path)?;
    let bytes = if canonical_json { jcs::canonicalize_json(&bytes)? } else { bytes };
    println!("{}", hash::hash(&bytes));
    Ok(())
}

fn command_id(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.first().map(String::as_str) != Some("object") {
        return Err("usage: veyra id object --fixture <name>".into());
    }
    let fixture = option_value(args, "--fixture")?;
    let universe = match option_value_optional(args, "--universe")? {
        Some(text) => UniverseId::parse(&text)?,
        None => UniverseId::fixture_sentinel(),
    };
    let address = ObjectAddress::Fixture { name: fixture };
    println!("{}", ObjectId::derive(universe, &address)?);
    Ok(())
}

fn command_cell(args: &[String]) -> Result<(), Box<dyn Error>> {
    let direction = option_value(args, "--dir")?;
    let level = option_value(args, "--level")?.parse::<u8>()?;
    let components: Vec<f64> =
        direction.split(',').map(str::parse::<f64>).collect::<Result<_, _>>()?;
    if components.len() != 3 {
        return Err("--dir requires x,y,z".into());
    }
    let direction = Dir::new(components[0], components[1], components[2])?;
    let key = DirCube.locate(direction, level)?;
    let (face, i, j, decoded_level) = DirCube::decode(key)?;
    println!("face={face} i={i} j={j} level={decoded_level} key=0x{:016x}", key.0);
    Ok(())
}

fn command_tile_key(args: &[String]) -> Result<(), Box<dyn Error>> {
    let topology = option_value(args, "--topology")?;
    let key = parse_cell_key(&option_value(args, "--cell")?)?;
    let tile_log2 = option_value(args, "--tile-log2")?.parse::<u8>()?;
    let tile = match topology.as_str() {
        "dir_cube" => DirCube.tile_key(key, tile_log2)?,
        "radial_1d" => Radial1d::default().tile_key(key, tile_log2)?,
        _ => return Err("topology must be dir_cube or radial_1d".into()),
    };
    println!("level={} address=0x{:016x}", tile.level, tile.address.0);
    Ok(())
}

fn command_body(args: &[String]) -> Result<(), Box<dyn Error>> {
    let action = args
        .first()
        .map(String::as_str)
        .ok_or("usage: veyra body verify|info|fields <directory>")?;
    let path = args.get(1).ok_or("missing body directory")?;
    match action {
        "verify" => println!("OK baseline=bas:{}", verify_directory(path)?),
        "info" => {
            let body = open_directory(path)?;
            let root = body.root_value();
            println!("object_id={}", body.object_id()?);
            println!("baseline_id=bas:{}", body.baseline_id());
            println!("fields={}", body.fields().len());
            println!("capabilities={}", root["capabilities"].as_array().map_or(0, Vec::len));
            println!("domains={}", root["domains"].as_array().map_or(0, Vec::len));
            println!("classification={}", root["classification"]);
        }
        "fields" => {
            let body = open_directory(path)?;
            println!("{}", serde_json::to_string_pretty(body.fields())?);
        }
        _ => return Err("body action must be verify, info, or fields".into()),
    }
    Ok(())
}

fn command_conformance(args: &[String]) -> Result<(), Box<dyn Error>> {
    let action = args.first().map(String::as_str).ok_or("usage: veyra conformance gen|verify")?;
    let root = repo_root();
    let worlds = args.get(1).map_or_else(|| root.join("conformance/worlds"), PathBuf::from);
    let vectors = args.get(2).map_or_else(|| root.join("conformance/vectors"), PathBuf::from);
    match action {
        "gen" => {
            veyra_conformance::generate_foundation(&worlds, &vectors)?;
            println!("generated foundation corpus at {}", worlds.display());
        }
        "verify" => {
            for result in veyra_conformance::verify_foundation(&worlds, &vectors)? {
                println!("PASS {} {}", result.name, result.evidence);
            }
        }
        _ => return Err("conformance action must be gen or verify".into()),
    }
    Ok(())
}

fn option_value(args: &[String], name: &str) -> Result<String, Box<dyn Error>> {
    option_value_optional(args, name)?.ok_or_else(|| format!("missing {name}").into())
}

fn option_value_optional(args: &[String], name: &str) -> Result<Option<String>, Box<dyn Error>> {
    let mut values = args.iter().filter(|argument| argument.as_str() == name);
    let Some(_) = values.next() else {
        return Ok(None);
    };
    let value = args
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| args.get(index + 1))
        .ok_or_else(|| format!("missing value for {name}"))?;
    if values.next().is_some() {
        return Err(format!("{name} may be supplied once").into());
    }
    Ok(Some(value.clone()))
}

fn parse_cell_key(text: &str) -> Result<CellKey, Box<dyn Error>> {
    let value = if let Some(hex) = text.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)?
    } else {
        u64::from_str(text)?
    };
    Ok(CellKey(value))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

fn print_help() {
    println!(
        "VEYRA foundation CLI\n\
         Commands:\n\
           fmt --canonical|--pretty <file>\n\
           hash <file>\n\
           hash-json <file>\n\
           id object --fixture <name> [--universe uni:b3:<hex>]\n\
           cell --dir x,y,z --level <0..30>\n\
           tile-key --topology dir_cube|radial_1d --cell <key> --tile-log2 <n>\n\
           body verify|info|fields <artifact-directory>\n\
           conformance gen|verify [world-root] [vector-root]"
    );
}

#[cfg(test)]
mod tests {
    use super::format_json;

    #[test]
    fn pretty_and_canonical_modes_reject_root_and_nested_duplicate_keys() {
        for duplicate in
            [br#"{"key":1,"key":2}"#.as_slice(), br#"{"outer":{"key":1,"key":2}}"#.as_slice()]
        {
            assert!(format_json(duplicate, "--pretty").is_err());
            assert!(format_json(duplicate, "--canonical").is_err());
        }
    }

    #[test]
    fn pretty_mode_accepts_normal_json_floats_and_repeated_array_values() {
        let output = format_json(br#"{"values":[1.5,1.5]}"#, "--pretty").unwrap();
        assert_eq!(output, b"{\n  \"values\": [\n    1.5,\n    1.5\n  ]\n}");
        assert!(format_json(br#"{"values":[1.5,1.5]}"#, "--canonical").is_err());
    }
}
