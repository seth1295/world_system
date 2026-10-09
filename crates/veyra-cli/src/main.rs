use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use veyra_core::body::FieldId;
use veyra_core::canon::{hash, jcs};
use veyra_core::ids::{ObjectAddress, ObjectId, UniverseId};
use veyra_core::sample::{LevelSel, Position, SampleQuery, TileRequest, TileView, TimeSel};
use veyra_core::spatial::{CellKey, Dir, DirCube, Radial1d, TileKey, Topology};
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
    if mode == "--canonical" {
        io::stdout().lock().write_all(&output)?;
    } else {
        println!("{}", String::from_utf8(output)?);
    }
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
        .ok_or("usage: veyra body verify|info|fields|sample|tile|inspect|views <directory>")?;
    match action {
        "verify" => {
            let path = args.get(1).ok_or("missing body directory")?;
            println!("OK baseline=bas:{}", verify_directory(path)?);
        }
        "info" => {
            let path = args.get(1).ok_or("missing body directory")?;
            let body = open_directory(path)?;
            let root = body.root_value();
            println!("object_id={}", body.object_id()?);
            println!("baseline_id=bas:{}", body.baseline_id());
            println!("fields={}", body.fields().len());
            println!("capabilities={}", body.capabilities().len());
            println!("domains={}", body.domains().len());
            println!("classification={}", root["classification"]);
        }
        "fields" => {
            let path = args.get(1).ok_or("missing body directory")?;
            let body = open_directory(path)?;
            println!("{}", serde_json::to_string_pretty(body.fields())?);
        }
        "views" => {
            let path = args.get(1).ok_or("missing body directory")?;
            let body = open_directory(path)?;
            println!("{}", serde_json::to_string_pretty(&body.views())?);
        }
        "sample" => {
            let path = args.get(1).ok_or("missing body directory")?;
            let body = open_directory(path)?;
            let query = parse_sample_query(&body, &args[2..])?;
            let sample = body.sample(&query)?;
            println!("{}", serde_json::to_string_pretty(&sample_json(&sample))?);
        }
        "inspect" => {
            let path = args.get(1).ok_or("missing body directory")?;
            let body = open_directory(path)?;
            let position = parse_position(&args[2..])?;
            let report =
                body.inspect(&position, parse_level(&args[2..])?, parse_time(&args[2..])?)?;
            let fields: Vec<_> = report
                .fields
                .iter()
                .map(|field| {
                    serde_json::json!({
                        "id":field.field.to_string(),"name":field.name,"semantic":field.semantic,
                        "unit":field.unit,"sample":field.sample.as_ref().map(sample_json),"issue":field.issue
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({"fields":fields}))?);
        }
        "tile" => {
            let path = args.get(1).ok_or("missing body directory")?;
            let body = open_directory(path)?;
            let field = parse_field_id(&body, &option_value(&args[2..], "--field")?)?;
            let (level, address) = parse_tile_key(&option_value(&args[2..], "--key")?)?;
            let key = TileKey { level, address };
            let halo = option_value_optional(&args[2..], "--halo")?
                .map(|value| value.parse::<u8>())
                .transpose()?
                .unwrap_or(0);
            let time = parse_time(&args[2..])?;
            let periodic =
                body.fields().iter().find(|descriptor| descriptor.id == field).is_some_and(
                    |descriptor| {
                        descriptor.temporal.get("kind").and_then(serde_json::Value::as_str)
                            == Some("periodic_slices")
                    },
                );
            let view = match option_value_optional(&args[2..], "--view")?.as_deref() {
                Some("raw") => TileView::Raw,
                Some("time_reduce") => TileView::TimeReduce,
                Some(view) if view.starts_with("derived:") => {
                    TileView::Derived(view[8..].to_owned())
                }
                Some(_) => return Err("--view must be raw, time_reduce, or derived:ID".into()),
                None if periodic && !matches!(time, TimeSel::Slice(_) | TimeSel::Phase(_)) => {
                    TileView::TimeReduce
                }
                None => TileView::Raw,
            };
            let request = TileRequest { field, key, time, halo, view };
            let tile = body.tile(&request)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "level":tile.key.level,"address":format!("0x{:016x}", tile.key.address.0),
                    "dim_i":tile.dim_i,"dim_j":tile.dim_j,"slices":tile.slices,
                    "source":format!("{:?}", tile.source),"values":tile.values
                }))?
            );
        }
        _ => {
            return Err(
                "body action must be verify, info, fields, sample, tile, inspect, or views".into(),
            );
        }
    }
    Ok(())
}

fn parse_sample_query(
    body: &veyra_core::io::Body,
    args: &[String],
) -> Result<SampleQuery, Box<dyn Error>> {
    Ok(SampleQuery {
        field: parse_field_id(body, &option_value(args, "--field")?)?,
        pos: parse_position(args)?,
        level: parse_level(args)?,
        time: parse_time(args)?,
    })
}

fn parse_field_id(body: &veyra_core::io::Body, text: &str) -> Result<FieldId, Box<dyn Error>> {
    if let Ok(field) = FieldId::parse(text) {
        return Ok(field);
    }
    let mut matches = body.fields().iter().filter(|field| field.name == text);
    let Some(field) = matches.next() else {
        return Err(format!("unknown field {text}").into());
    };
    if matches.next().is_some() {
        return Err(format!("field name {text} is ambiguous; select by FieldId").into());
    }
    Ok(field.id)
}

fn parse_position(args: &[String]) -> Result<Position, Box<dyn Error>> {
    let mut found = Vec::new();
    for (option, kind) in [
        ("--dir", "direction"),
        ("--axial-latlon", "axial"),
        ("--cell", "cell"),
        ("--radius", "radius"),
    ] {
        if let Some(value) = option_value_optional(args, option)? {
            found.push((kind, value));
        }
    }
    if found.len() != 1 {
        return Err("provide exactly one of --dir, --axial-latlon, --cell, or --radius".into());
    }
    let (kind, value) = &found[0];
    match *kind {
        "direction" => {
            let parts: Vec<f64> = value.split(',').map(str::parse).collect::<Result<_, _>>()?;
            if parts.len() != 3 {
                return Err("--dir requires x,y,z".into());
            }
            Ok(Position::Direction(Dir::new(parts[0], parts[1], parts[2])?))
        }
        "axial" => {
            let parts: Vec<f64> = value.split(',').map(str::parse).collect::<Result<_, _>>()?;
            if parts.len() != 2 {
                return Err("--axial-latlon requires latitude,longitude in degrees".into());
            }
            let radians = core::f64::consts::PI / 180.0;
            Ok(Position::AxialLatLon { lat_rad: parts[0] * radians, lon_rad: parts[1] * radians })
        }
        "cell" => {
            let (domain, key) = value.split_once(':').ok_or("--cell requires DOMAIN:KEY")?;
            Ok(Position::Cell { domain: domain.to_owned(), key: parse_cell_key(key)? })
        }
        "radius" => Ok(Position::Radial { r_m: value.parse()? }),
        _ => Err("invalid position selector".into()),
    }
}

fn parse_level(args: &[String]) -> Result<LevelSel, Box<dyn Error>> {
    match option_value_optional(args, "--level")?.as_deref().unwrap_or("native") {
        "native" => Ok(LevelSel::Native),
        "canonical" => Ok(LevelSel::Canonical),
        value => Ok(LevelSel::Exact(value.parse()?)),
    }
}

fn parse_time(args: &[String]) -> Result<TimeSel, Box<dyn Error>> {
    let slice = option_value_optional(args, "--slice")?;
    let time = option_value_optional(args, "--time")?;
    if slice.is_some() && time.is_some() {
        return Err("--slice and --time are mutually exclusive".into());
    }
    if let Some(slice) = slice {
        if slice == "mean" {
            return Ok(TimeSel::Mean);
        }
        return Ok(TimeSel::Slice(slice.parse()?));
    }
    match time.as_deref().unwrap_or("static") {
        "static" => Ok(TimeSel::Static),
        "mean" => Ok(TimeSel::Mean),
        "min" => Ok(TimeSel::Min),
        "max" => Ok(TimeSel::Max),
        value if value.starts_with("phase:") => Ok(TimeSel::Phase(value[6..].parse()?)),
        value if value.starts_with("slice:") => Ok(TimeSel::Slice(value[6..].parse()?)),
        _ => Err("--time must be static, mean, min, max, phase:N, or slice:N".into()),
    }
}

fn parse_tile_key(text: &str) -> Result<(u8, CellKey), Box<dyn Error>> {
    let (level, key) = text.split_once(':').ok_or("--key requires LEVEL:KEY")?;
    Ok((level.parse()?, parse_cell_key(key)?))
}

fn sample_json(sample: &veyra_core::sample::Sample) -> serde_json::Value {
    serde_json::json!({
        "value":sample.value,
        "raw":sample.raw.map(|raw| match raw {
            veyra_core::sample::RawValue::Integer(value) => serde_json::json!(value),
            veyra_core::sample::RawValue::Float(value) => serde_json::json!(value),
        }),
        "category":sample.category,
        "level_used":sample.level_used,
        "source":format!("{:?}", sample.source),
        "cell":format!("0x{:016x}", sample.cell.0),
    })
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
        "VEYRA World System CLI\n\
         Commands:\n\
           fmt --canonical|--pretty <file>\n\
           hash <file>\n\
           hash-json <file>\n\
           id object --fixture <name> [--universe uni:b3:<hex>]\n\
           cell --dir x,y,z --level <0..30>\n\
           tile-key --topology dir_cube|radial_1d --cell <key> --tile-log2 <n>\n\
           body verify|info|fields|views <artifact-directory>\n\
           body sample|inspect <artifact-directory> --field <id|name> <position> [--level native|canonical|N] [--slice K|--time mean|min|max|phase:N]\n\
           body tile <artifact-directory> --field <id|name> --key LEVEL:KEY [--halo 0|1] [--time selection] [--view raw|time_reduce|derived:ID]\n\
           conformance gen|verify [world-root] [vector-root]"
    );
}

#[cfg(test)]
mod tests {
    use super::{format_json, parse_time};
    use veyra_core::sample::TimeSel;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

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

    #[test]
    fn time_selector_parser_rejects_conflicts_and_preserves_valid_forms() {
        for conflicting in [
            args(&["--slice", "1", "--time", "min"]),
            args(&["--time", "min", "--slice", "1"]),
            args(&["--slice", "1", "--time", "invalid"]),
            args(&["--time", "invalid", "--slice", "1"]),
            args(&["--slice", "invalid", "--time", "min"]),
        ] {
            assert!(parse_time(&conflicting).is_err(), "{conflicting:?}");
        }

        assert_eq!(parse_time(&args(&["--slice", "1"])).unwrap(), TimeSel::Slice(1));
        assert_eq!(parse_time(&args(&["--slice", "mean"])).unwrap(), TimeSel::Mean);
        for (value, expected) in [
            ("static", TimeSel::Static),
            ("mean", TimeSel::Mean),
            ("min", TimeSel::Min),
            ("max", TimeSel::Max),
            ("phase:0.25", TimeSel::Phase(0.25)),
            ("slice:2", TimeSel::Slice(2)),
        ] {
            assert_eq!(parse_time(&args(&["--time", value])).unwrap(), expected);
        }
    }
}
