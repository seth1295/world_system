#!/usr/bin/env python3
"""Validate JSON schema documents, capability schemas, and committed body fixtures."""

from __future__ import annotations

from copy import deepcopy
import json
import tomllib
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schema" / "json"
CAPABILITIES = ROOT / "schema" / "capabilities"
FIXTURES = ROOT / "conformance" / "worlds"


def reject_duplicate_names(pairs):
    value = {}
    for name, item in pairs:
        if name in value:
            raise ValueError(f"duplicate JSON object name: {name}")
        value[name] = item
    return value


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_names)


def set_path(value, path, replacement):
    target = value
    for key in path[:-1]:
        target = target[key]
    target[path[-1]] = replacement


def remove_path(value, path):
    target = value
    for key in path[:-1]:
        target = target[key]
    target.pop(path[-1], None)


def check_cases(validator, cases, label):
    for name, instance, expected_valid in cases:
        actual_valid = validator.is_valid(instance)
        if actual_valid != expected_valid:
            expected = "accepted" if expected_valid else "rejected"
            raise SystemExit(f"{label} parity case {name}: expected schema to be {expected}")


def main() -> int:
    try:
        json.loads('{"nested":{"key":1,"key":2}}', object_pairs_hook=reject_duplicate_names)
    except ValueError:
        pass
    else:
        raise SystemExit("schema harness duplicate-key parser accepted a duplicate nested object name")

    schemas = {path.name: read_json(path) for path in SCHEMAS.glob("*.json")}
    schema_registry = Registry()
    for schema in schemas.values():
        Draft202012Validator.check_schema(schema)
        schema_registry = schema_registry.with_resource(
            schema["$id"], Resource.from_contents(schema)
        )
    capability_schema = schemas["capability_document.json"]
    body_schema = schemas["body.json"]
    field_schema = schemas["field_descriptor.json"]
    registry_schema = schemas["field_registry.json"]
    capability_validator = Draft202012Validator(capability_schema, registry=schema_registry)
    body_validator = Draft202012Validator(body_schema, registry=schema_registry)
    registry_validator = Draft202012Validator(registry_schema, registry=schema_registry)
    field_validator = Draft202012Validator(field_schema, registry=schema_registry)
    decimal_validator = Draft202012Validator(
        {"$ref": "https://veyra.invalid/schema/decimal/1#/$defs/decimalString"},
        registry=schema_registry,
    )
    positive_decimal_validator = Draft202012Validator(
        {"$ref": "https://veyra.invalid/schema/decimal/1#/$defs/positiveDecimalString"},
        registry=schema_registry,
    )
    utime_validator = Draft202012Validator(
        {"$ref": "https://veyra.invalid/schema/decimal/1#/$defs/utime"},
        registry=schema_registry,
    )

    # Paired with DecimalString::parse tests in time.rs. The effective exponent uses i64,
    # so every canonical i32 exponent token remains accepted regardless of fraction length.
    decimal_cases = [
        ("i32 maximum", "1e2147483647", True),
        ("i32 minimum", "1e-2147483648", True),
        ("fractional i32 minimum", "1.0e-2147483648", True),
        ("overflow positive", "1e2147483648", False),
        ("overflow negative", "1e-2147483649", False),
        ("negative zero exponent", "1e-0", False),
        ("leading-zero exponent", "1e00", False),
        ("plus exponent", "1e+1", False),
        ("uppercase exponent", "1E3", False),
        ("empty exponent", "1e", False),
    ]
    check_cases(decimal_validator, decimal_cases, "DecimalString")
    positive_cases = [
        ("positive integer", "1", True),
        ("positive fraction", "0.1", True),
        ("positive exponent", "1.25e-3", True),
        ("positive maximum exponent", "1e2147483647", True),
        ("positive minimum exponent", "1e-2147483648", True),
        ("positive fractional minimum exponent", "1.0e-2147483648", True),
        ("zero", "0", False),
        ("fractional zero", "0.0", False),
        ("exponent zero", "0e3", False),
        ("fractional exponent zero", "0.0e-3", False),
        ("negative", "-1", False),
        ("negative zero", "-0", False),
        ("leading zero", "01", False),
        ("exponent above i32", "1e2147483648", False),
        ("exponent below i32", "1e-2147483649", False),
    ]
    check_cases(positive_decimal_validator, positive_cases, "positive DecimalString")
    utime_cases = [
        ("zero", "0", True),
        ("negative one", "-1", True),
        ("i128 maximum", "170141183460469231731687303715884105727", True),
        ("i128 minimum", "-170141183460469231731687303715884105728", True),
        ("above i128 maximum", "170141183460469231731687303715884105728", False),
        ("below i128 minimum", "-170141183460469231731687303715884105729", False),
        ("negative zero", "-0", False),
        ("leading zero", "01", False),
        ("exponent form", "1e3", False),
        ("plus sign", "+1", False),
    ]
    check_cases(utime_validator, utime_cases, "UTime")

    body_fixture = read_json(FIXTURES / "cb9-minimal-void" / "body.json")
    gm_cases = []
    for name, value, valid in positive_cases:
        instance = deepcopy(body_fixture)
        instance["physical"]["gm_m3_s2"] = value
        gm_cases.append((name, instance, valid))
    check_cases(body_validator, gm_cases, "body physical GM")

    # Figure case names are mirrored by body::tests::figure_kind_parameters_are_required_and_positive.
    figure_cases = []
    for name, figure, valid in [
        ("sphere valid", {"kind": "sphere", "radius_m": "2"}, True),
        ("star-convex valid", {"kind": "star_convex_radial", "radius_field": "figure.radius_m"}, True),
        ("radial-profile valid", {"kind": "radial_profile_sphere", "extent_m": "2"}, True),
        ("sphere extra parameter remains forward-compatible", {"kind": "sphere", "radius_m": "2", "extent_m": "3"}, True),
        ("sphere missing radius", {"kind": "sphere"}, False),
        ("sphere wrong radius type", {"kind": "sphere", "radius_m": 2}, False),
        ("sphere zero radius", {"kind": "sphere", "radius_m": "0"}, False),
        ("sphere negative radius", {"kind": "sphere", "radius_m": "-1"}, False),
        ("sphere malformed radius", {"kind": "sphere", "radius_m": "01"}, False),
        ("star-convex missing radius field", {"kind": "star_convex_radial"}, False),
        ("star-convex empty radius field", {"kind": "star_convex_radial", "radius_field": ""}, False),
        ("star-convex wrong radius field type", {"kind": "star_convex_radial", "radius_field": 1}, False),
        ("radial-profile missing extent", {"kind": "radial_profile_sphere"}, False),
        ("radial-profile zero extent", {"kind": "radial_profile_sphere", "extent_m": "0.0"}, False),
        ("radial-profile wrong extent type", {"kind": "radial_profile_sphere", "extent_m": 2}, False),
        ("reserved figure", {"kind": "ellipsoid", "radius_m": "2"}, False),
    ]:
        instance = deepcopy(body_fixture)
        instance["figure"] = figure
        figure_cases.append((name, instance, valid))
    check_cases(body_validator, figure_cases, "figure contract")

    # Domain case names are mirrored by body::tests::radial_domain_extent_uses_the_same_positive_decimal_rule_as_gm.
    radial_base = deepcopy(body_fixture)
    radial_base["required_features"].append("veyra.topo.radial_1d/1")
    radial_base["figure"] = {"kind": "radial_profile_sphere", "extent_m": "2"}
    radial_base["reference_surfaces"] = [{"id": "photosphere", "kind": "sphere", "radius_m": "2"}]
    radial_base["domains"] = [{
        "id": "interior", "topology": "veyra.topo.radial_1d/1", "frame": "body_fixed",
        "vertical": {"kind": "radius", "extent_m": "2"}, "tile_log2": 2, "max_level": 5,
    }]
    radial_domain_cases = []
    for name, extent, valid in [
        ("radial extent positive integer", "2", True),
        ("radial extent positive fraction", "0.1", True),
        ("radial extent positive exponent", "1.25e-3", True),
        ("radial extent i32 boundary", "1e-2147483648", True),
        ("radial extent zero", "0", False),
        ("radial extent decimal zero", "0.0", False),
        ("radial extent exponent zero", "0e3", False),
        ("radial extent fractional exponent zero", "0.0e-3", False),
        ("radial extent negative", "-1", False),
        ("radial extent malformed", "01", False),
        ("radial extent exponent overflow", "1e2147483648", False),
        ("radial extent exponent underflow", "1e-2147483649", False),
    ]:
        instance = deepcopy(radial_base)
        instance["domains"][0]["vertical"]["extent_m"] = extent
        radial_domain_cases.append((name, instance, valid))
    check_cases(body_validator, radial_domain_cases, "radial domain")

    domain_shape_cases = []
    for name, key, replacement in [
        ("empty domain id", "id", ""),
        ("unknown topology", "topology", "veyra.topo.future/1"),
        ("unsupported frame", "frame", "universe_inertial"),
        ("tile_log2 above V1", "tile_log2", 31),
        ("max_level above V1", "max_level", 31),
    ]:
        instance = deepcopy(radial_base)
        instance["domains"][0][key] = replacement
        domain_shape_cases.append((name, instance, False))
    instance = deepcopy(radial_base)
    instance["domains"][0]["vertical"]["kind"] = "none"
    domain_shape_cases.append(("radial wrong vertical kind", instance, False))
    cube_wrong_vertical = deepcopy(body_fixture)
    cube_wrong_vertical["required_features"].append("veyra.topo.dir_cube/1")
    cube_wrong_vertical["domains"] = [{
        "id": "surface", "topology": "veyra.topo.dir_cube/1", "frame": "body_fixed",
        "vertical": {"kind": "radius", "extent_m": "1"}, "tile_log2": 2, "max_level": 5,
    }]
    domain_shape_cases.append(("cube wrong vertical kind", cube_wrong_vertical, False))
    check_cases(body_validator, domain_shape_cases, "domain structural contract")

    # Schema cannot compare IDs across array entries. These cases are intentionally
    # accepted here and rejected by Rust's BTreeSet-based duplicate/reference checks.
    duplicate_domains = deepcopy(body_fixture)
    duplicate_domains["required_features"].extend(["veyra.topo.dir_cube/1", "veyra.topo.radial_1d/1"])
    duplicate_domains["domains"] = [
        {"id": "same", "topology": "veyra.topo.dir_cube/1", "frame": "body_fixed", "vertical": {"kind": "none"}, "tile_log2": 2, "max_level": 5},
        {"id": "same", "topology": "veyra.topo.radial_1d/1", "frame": "body_fixed", "vertical": {"kind": "radius", "extent_m": "1"}, "tile_log2": 2, "max_level": 5},
    ]
    check_cases(body_validator, [("duplicate domain ids are Rust-only", duplicate_domains, True)], "cross-reference boundary")
    missing_topology_feature = deepcopy(radial_base)
    missing_topology_feature["required_features"].remove("veyra.topo.radial_1d/1")
    check_cases(
        body_validator,
        [("domain topology feature presence is Rust-only cross-reference", missing_topology_feature, True)],
        "cross-reference boundary",
    )

    frame_fixture = deepcopy(body_fixture)
    frame_cases = []
    for frame in (None, 1, "body_fixed"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"] = frame
        frame_cases.append(("frame shape", body, False))
    body = deepcopy(frame_fixture)
    body["frames"]["body_fixed"].pop("axes")
    frame_cases.append(("missing axes", body, False))
    for axes in ("", "right-handed"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["axes"] = axes
        frame_cases.append(("invalid axes", body, False))
    body = deepcopy(frame_fixture)
    body["frames"]["body_fixed"].pop("rotation")
    frame_cases.append(("missing rotation", body, False))
    for key in ("kind", "period_s", "epoch", "orientation_q_at_epoch", "relative_to"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"].pop(key)
        frame_cases.append((f"missing rotation {key}", body, False))
    for rotation in (None, 1, "uniform"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"] = rotation
        frame_cases.append(("rotation shape", body, False))
    for key, value in (
        ("kind", "precessing"),
        ("period_s", "not-a-period"),
        ("period_s", "0"),
        ("epoch", "00"),
        ("epoch", "170141183460469231731687303715884105728"),
        ("relative_to", "unknown_frame"),
    ):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"][key] = value
        frame_cases.append((f"invalid rotation {key}", body, False))
    for epoch in (
        "170141183460469231731687303715884105727",
        "-170141183460469231731687303715884105728",
    ):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"]["epoch"] = epoch
        frame_cases.append(("canonical UTime i128 boundary", body, True))
    for quaternion in (["1", "0", "0"], ["1", "0", "NaN", "0"], ["1", "0", 0, "0"]):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"]["orientation_q_at_epoch"] = quaternion
        frame_cases.append(("invalid orientation quaternion", body, False))
    # Unit norm and finite binary64 conversion are semantic checks kept in Rust.
    body = deepcopy(frame_fixture)
    body["frames"]["body_fixed"]["rotation"]["orientation_q_at_epoch"] = ["2", "0", "0", "0"]
    frame_cases.append(("quaternion unit norm is Rust-only", body, True))
    body = deepcopy(frame_fixture)
    body["frames"]["body_fixed"]["rotation"]["orientation_q_at_epoch"] = ["1e9999", "0", "0", "0"]
    frame_cases.append(("quaternion finite f64 range is Rust-only", body, True))
    body = deepcopy(frame_fixture)
    body["required_features"].append("veyra.topo.dir_cube/1")
    body["domains"] = [{
        "id": "surface", "topology": "veyra.topo.dir_cube/1", "frame": "missing_frame",
        "vertical": {"kind": "none"}, "tile_log2": 2, "max_level": 5,
    }]
    frame_cases.append(("unsupported domain frame", body, False))
    check_cases(body_validator, frame_cases, "body-fixed frame")

    surface_cases = []
    for name, figure, surfaces, valid in [
        ("radial profile photosphere", {"kind": "radial_profile_sphere", "extent_m": "2"}, [{"id": "photosphere", "kind": "sphere", "radius_m": "2"}], True),
        ("sphere figure surface", {"kind": "sphere", "radius_m": "2"}, [{"id": "solid.boundary", "kind": "figure_surface"}], True),
        ("star-convex figure surface", {"kind": "star_convex_radial", "radius_field": "figure.radius_m"}, [{"id": "solid.boundary", "kind": "figure_surface"}], True),
        ("radial figure surface", {"kind": "radial_profile_sphere", "extent_m": "2"}, [{"id": "figure.boundary", "kind": "figure_surface"}], False),
        ("reserved surface kind", {"kind": "sphere", "radius_m": "2"}, [{"id": "reserved", "kind": "ellipsoid"}], False),
        ("sphere surface missing radius", {"kind": "sphere", "radius_m": "2"}, [{"id": "datum", "kind": "sphere"}], False),
        ("offset surface missing base", {"kind": "sphere", "radius_m": "2"}, [{"id": "offset", "kind": "offset_of", "offset_m": "0"}], False),
        ("offset surface malformed decimal", {"kind": "sphere", "radius_m": "2"}, [{"id": "offset", "kind": "offset_of", "base": "datum", "offset_m": "1e2147483648"}], False),
        ("sphere radius exponent overflow", {"kind": "sphere", "radius_m": "2"}, [{"id": "datum", "kind": "sphere", "radius_m": "1e2147483648"}], False),
        ("offset missing base target is Rust-only", {"kind": "sphere", "radius_m": "2"}, [{"id": "offset", "kind": "offset_of", "base": "missing", "offset_m": "1"}], True),
        ("offset cycle is Rust-only", {"kind": "sphere", "radius_m": "2"}, [{"id": "a", "kind": "offset_of", "base": "b", "offset_m": "1"}, {"id": "b", "kind": "offset_of", "base": "a", "offset_m": "1"}], True),
        ("duplicate surface ids are Rust-only", {"kind": "sphere", "radius_m": "2"}, [{"id": "same", "kind": "sphere", "radius_m": "1"}, {"id": "same", "kind": "sphere", "radius_m": "2"}], True),
    ]:
        instance = deepcopy(body_fixture)
        instance["figure"] = figure
        instance["reference_surfaces"] = surfaces
        surface_cases.append((name, instance, valid))
    check_cases(body_validator, surface_cases, "reference surface contract")

    # Pair field schema cases with the Rust scalar-unit and metadata validation tests.
    field_base = {
        "id": "0x01010001", "name": "topography.height_m",
        "capability": "veyra.cap.topography/1", "domain": "surface",
        "semantic": "scalar.height", "persistence": "invariant",
        "storage": {"dtype": "i16", "scale": "0.5", "offset": "0"},
        "native_level": 3, "temporal": {"kind": "static"},
        "sampling": {"interp": "bilinear", "below_native": "pyramid", "above_native": "refine"},
        "downsample": "mean", "compat": "critical", "unit": "m",
    }
    field_cases = [("critical scalar with unit", deepcopy(field_base), True)]
    reserved_local_id = deepcopy(field_base)
    reserved_local_id["id"] = "0x01010000"
    field_cases.append(("reserved zero field-local id", reserved_local_id, False))
    unallocated_capability_id = deepcopy(field_base)
    unallocated_capability_id["id"] = "0x00000001"
    field_cases.append(("reserved zero capability id", unallocated_capability_id, False))
    for name, change in [
        ("critical scalar missing unit", None),
        ("critical scalar empty unit", ""),
        ("critical scalar null unit", None),
        ("critical scalar non-string unit", 5),
    ]:
        instance = deepcopy(field_base)
        if name.endswith("missing unit"):
            instance.pop("unit")
        else:
            instance["unit"] = change
        field_cases.append((name, instance, False))
    category = deepcopy(field_base)
    category["semantic"] = "category"
    category["storage"]["dtype"] = "u8"
    category.pop("unit")
    field_cases.append(("non-scalar critical field needs no unit", category, True))
    ancillary = deepcopy(field_base)
    ancillary["compat"] = "ancillary"
    ancillary["semantic"] = "scalar.future_quantity"
    ancillary["storage"] = {"dtype": "future_dtype", "scale": 7}
    ancillary.pop("unit")
    ancillary.pop("temporal")
    ancillary.pop("sampling")
    ancillary.pop("downsample")
    field_cases.append(("ancillary forward-compatible scalar metadata", ancillary, True))
    for value, valid in [
        ("1e2147483647", True),
        ("1e-2147483648", True),
        ("1.0e-2147483648", True),
        ("1e2147483648", False),
        ("1e-2147483649", False),
        ("1e-0", False),
        ("1e00", False),
    ]:
        instance = deepcopy(field_base)
        instance["storage"]["scale"] = value
        field_cases.append((f"critical storage decimal {value}", instance, valid))
    for name, key, value in [
        ("unsupported interpolation", "sampling", {"interp": "cubic"}),
        ("unsupported downsample", "downsample", "median"),
        ("reserved temporal series", "temporal", {"kind": "series"}),
        ("static temporal extra semantic", "temporal", {"kind": "static", "count": 4}),
        ("periodic temporal missing refs", "temporal", {"kind": "periodic_slices", "count": 4}),
        ("periodic temporal count zero", "temporal", {"kind": "periodic_slices", "count": 0, "period_ref": "dynamics.period", "origin_ref": "dynamics.epoch"}),
        ("category with f32 storage", "semantic", "category"),
        ("critical unknown semantic", "semantic", "x-future.semantic/1"),
    ]:
        instance = deepcopy(field_base)
        if key == "semantic" and value == "category":
            instance["storage"]["dtype"] = "f32"
        instance[key] = value
        field_cases.append((name, instance, False))
    check_cases(field_validator, field_cases, "field descriptor")

    # Dtype-specific nodata bounds are schema-expressible and share the Rust bounds tests.
    dtype_nodata_cases = []
    dtype_limits = {
        "u8": (0, 255), "i8": (-128, 127), "u16": (0, 65535),
        "i16": (-32768, 32767), "u32": (0, 4294967295),
        "i32": (-2147483648, 2147483647), "f32": (0, 4294967295),
    }
    for dtype, (minimum, maximum) in dtype_limits.items():
        for label, nodata, valid in (("minimum", minimum, True), ("maximum", maximum, True), ("underflow", minimum - 1, False), ("overflow", maximum + 1, False)):
            instance = deepcopy(field_base)
            instance["storage"]["dtype"] = dtype
            instance["storage"]["nodata"] = nodata
            dtype_nodata_cases.append((f"{dtype} nodata {label}", instance, valid))
    check_cases(field_validator, dtype_nodata_cases, "critical nodata bounds")

    duplicate_field_registry = {"schema": "veyra.field_registry/1", "fields": [deepcopy(field_base), deepcopy(field_base)]}
    check_cases(
        registry_validator,
        [("duplicate FieldId is Rust-only", duplicate_field_registry, True)],
        "field registry cross-entry boundary",
    )

    # Dynamic fields are valid descriptors in isolation but forbidden in a body registry.
    dynamic_descriptor = deepcopy(ancillary)
    dynamic_descriptor["persistence"] = "dynamic"
    if not field_validator.is_valid(dynamic_descriptor):
        raise SystemExit("field schema rejected a forward-compatible dynamic descriptor outside a baseline registry")
    dynamic_registry = {"schema": "veyra.field_registry/1", "fields": [dynamic_descriptor]}
    if registry_validator.is_valid(dynamic_registry):
        raise SystemExit("field registry schema accepted a dynamic baseline descriptor")

    # These body/reference shapes are schema-expressible; duplicate keys/IDs and semantic
    # references are intentionally Rust-only and are paired with Rust validation tests.
    body_structural_cases = []
    for name, mutate, valid in [
        ("missing mandatory feature", lambda item: item["required_features"].remove("veyra.body/1"), False),
        ("empty required feature", lambda item: item["required_features"].append(""), False),
        ("unsupported format major", lambda item: item["format_version"].update(major=2), False),
        ("fractional integer lexical form is JCS-only", lambda item: item["format_version"].update(major=1.0), True),
        ("minor above serde u16", lambda item: item["format_version"].update(minor=65536), False),
        ("malformed object id", lambda item: item["identity"].update(object_id="obj:bad"), False),
        ("unsupported origin kind", lambda item: item["identity"].update(origin={"kind": "recipe", "id": "x"}), False),
        ("fixture origin missing name", lambda item: item["identity"].update(origin={"kind": "fixture"}), False),
        ("fixture origin empty name", lambda item: item["identity"].update(origin={"kind": "fixture", "name": ""}), False),
        ("universe origin malformed ID", lambda item: item["identity"].update(origin={"kind": "universe", "universe_id": "uni:b3:bad", "address": {"kind": "fixture", "name": "x"}}), False),
        ("universe origin malformed address", lambda item: item["identity"].update(origin={"kind": "universe", "universe_id": "uni:b3:" + "0" * 64, "address": {"kind": "future", "name": "x"}}), False),
        ("fractional region level notation is JCS-only", lambda item: item["identity"].update(origin={"kind": "universe", "universe_id": "uni:b3:" + "0" * 64, "address": {"kind": "system_seed", "region": {"level": 0.0, "ix": 0, "iy": 0, "iz": 0}, "slot": 0}}), True),
        ("invalid index key", lambda item: item["indexes"].update({"0X01010001": "b3:" + "0" * 64}), False),
        ("reserved zero index-local id", lambda item: item["indexes"].update({"0x01010000": "b3:" + "0" * 64}), False),
        ("reserved zero index capability id", lambda item: item["indexes"].update({"0x00000001": "b3:" + "0" * 64}), False),
        ("invalid index hash", lambda item: item["indexes"].update({"0x01010001": "bad-hash"}), False),
        ("empty critical capability id", lambda item: item.update(capabilities=[{"id": "", "params": {}}]), False),
        ("non-object capability params", lambda item: item.update(capabilities=[{"id": "x-future/1", "params": 7, "compat": "ancillary"}]), False),
        ("unknown ancillary body extensions preserved", lambda item: item.update(**{"x-future": {"v": 1}}), True),
    ]:
        instance = deepcopy(body_fixture)
        mutate(instance)
        body_structural_cases.append((name, instance, valid))
    # ObjectId derivation and some body-local references are content/cross-field checks.
    mismatched_id = deepcopy(body_fixture)
    mismatched_id["identity"]["origin"]["name"] = "different-fixture-name"
    body_structural_cases.append(("object id derives from origin (Rust-only)", mismatched_id, True))
    universe_origin = deepcopy(body_fixture)
    universe_origin["identity"]["origin"] = {
        "kind": "universe", "universe_id": "uni:b3:" + "0" * 64,
        "address": {"kind": "system_seed", "region": {"ix": 0, "iy": 0, "iz": 0}, "slot": 0},
    }
    body_structural_cases.append(("universe address ObjectId derivation (Rust-only)", universe_origin, True))
    large_region_string = deepcopy(body_fixture)
    large_region_string["identity"]["origin"] = {
        "kind": "universe", "universe_id": "uni:b3:" + "0" * 64,
        "address": {"kind": "system_seed", "region": {"ix": "9007199254740992", "iy": "-9007199254740993", "iz": 0}, "slot": 0},
    }
    body_structural_cases.append(("large region coordinates use decimal strings", large_region_string, True))
    unsafe_region_number = deepcopy(body_fixture)
    unsafe_region_number["identity"]["origin"] = {
        "kind": "universe", "universe_id": "uni:b3:" + "0" * 64,
        "address": {"kind": "system_seed", "region": {"ix": 9007199254740992, "iy": 0, "iz": 0}, "slot": 0},
    }
    body_structural_cases.append(("unsafe region JSON integer", unsafe_region_number, False))
    region_string_overflow = deepcopy(body_fixture)
    region_string_overflow["identity"]["origin"] = {
        "kind": "universe", "universe_id": "uni:b3:" + "0" * 64,
        "address": {"kind": "system_seed", "region": {"ix": "9223372036854775808", "iy": 0, "iz": 0}, "slot": 0},
    }
    body_structural_cases.append(("region decimal string beyond i64", region_string_overflow, False))
    for coordinate, valid in [
        ("9223372036854775807", True),
        ("-9223372036854775808", True),
        ("9223372036854775808", False),
        ("-9223372036854775809", False),
    ]:
        instance = deepcopy(body_fixture)
        instance["identity"]["origin"] = {
            "kind": "universe", "universe_id": "uni:b3:" + "0" * 64,
            "address": {"kind": "system_seed", "region": {"ix": coordinate, "iy": 0, "iz": 0}, "slot": 0},
        }
        body_structural_cases.append((f"i64 region coordinate {coordinate}", instance, valid))
    unregistered_index = deepcopy(body_fixture)
    unregistered_index["indexes"] = {"0x01000001": "b3:" + "0" * 64}
    body_structural_cases.append(("index-to-registry field relation (Rust-only)", unregistered_index, True))
    check_cases(body_validator, body_structural_cases, "body structural contract")

    # Section paths are structurally expressible and share the core artifact-path grammar.
    section_cases = []
    for path, valid in [
        ("registry/fields.json", True), ("", False), ("/x", False), ("/absolute", False),
        ("body.json", False), ("body.id", False),
        ("a/./b", False), ("a//b", False), ("../x", False), ("a/../x", False),
        ("a\\b", False), ("C:/x", False), ("a:b", False), ("a/", False),
        ("C:relative", False), ("a/C:/x", False),
        ("a\x00b", False), ("a\x01b", False), ("a\x1fb", False),
        ("bad<name", False), ("bad>name", False), ('bad"name', False),
        ("bad|name", False), ("bad?name", False), ("bad*name", False),
        ("trailing ", False), ("trailing.", False), ("dir./file", False),
        ("dir /file", False),
        ("CON", False), ("con", False), ("Con.txt", False),
        ("NUL", False), ("nul.json", False), ("PRN", False), ("AUX", False),
        ("COM1", False), ("com9.bin", False), ("LPT1", False), ("lpt9.data", False),
        ("registry/NUL", False), ("registry/COM1.txt", False),
        ("COM¹", False), ("COM².ext", False), ("LPT³", False), ("LPT³.foo", False),
        ("console", True), ("null", True), ("COM0", True), ("COM10", True),
        ("LPT0", True), ("LPT10", True), ("xCON", True), ("CONx", True),
        ("company.txt", True), (".hidden", True), ("café/世界.json", True),
        ("delete\u007fcharacter", True),
    ]:
        instance = deepcopy(body_fixture)
        instance["sections"]["registry"]["path"] = path
        section_cases.append((f"registry path {path!r}", instance, valid))
    for control in range(1, 32):
        path = f"file{chr(control)}name"
        instance = deepcopy(body_fixture)
        instance["sections"]["registry"]["path"] = path
        section_cases.append((f"registry path with ASCII control U+{control:04X}", instance, False))
    for prefix in ("COM", "LPT"):
        for digit in range(1, 10):
            for basename in (f"{prefix}{digit}", f"{prefix}{digit}.data"):
                instance = deepcopy(body_fixture)
                instance["sections"]["registry"]["path"] = basename
                section_cases.append((f"reserved device path {basename}", instance, False))
        for digit in ("\u00b9", "\u00b2", "\u00b3"):
            for basename in (f"{prefix}{digit}", f"{prefix}{digit}.data"):
                instance = deepcopy(body_fixture)
                instance["sections"]["registry"]["path"] = basename
                section_cases.append((f"reserved superscript device path {basename}", instance, False))
    check_cases(body_validator, section_cases, "section path")

    # Schema cannot express uniqueBy(domain.id), capability dependencies, or cross-section
    # ObjectId/hash/content checks. Duplicate domain IDs are intentionally Rust-only.
    duplicate_capabilities = deepcopy(body_fixture)
    duplicate_capabilities["capabilities"] = [
        {"id": "x-future/1", "params": {}, "compat": "ancillary"},
        {"id": "x-future/1", "params": {}, "compat": "ancillary", "x-extra": 1},
    ]
    check_cases(body_validator, [("duplicate capability ids are Rust-only", duplicate_capabilities, True)], "cross-reference boundary")

    optional_nulls = deepcopy(body_fixture)
    optional_nulls["identity"]["label"] = None
    optional_nulls["physical"]["gravity_model"] = None
    optional_nulls["sections"]["provenance"] = None
    optional_nulls["extensions_ledger"] = None
    check_cases(body_validator, [("optional serde nulls", optional_nulls, True)], "optional-value parity")

    capability_doc = read_json(CAPABILITIES / "topography.json")
    capability_doc_cases = [("declared V1 capability", deepcopy(capability_doc), True)]
    invalid_doc = deepcopy(capability_doc)
    invalid_doc["requires"] = [""]
    capability_doc_cases.append(("empty dependency identifier", invalid_doc, False))
    invalid_doc = deepcopy(capability_doc)
    invalid_doc["field_templates"][0]["local_id"] = 0
    capability_doc_cases.append(("field template local id zero", invalid_doc, False))
    invalid_doc = deepcopy(capability_doc)
    invalid_doc["field_templates"][0]["local_id"] = 65536
    capability_doc_cases.append(("field template local id above u16", invalid_doc, False))
    check_cases(capability_validator, capability_doc_cases, "capability document structure")

    named_refs_body = deepcopy(body_fixture)
    named_refs_body["sections"]["vocab"] = [
        {"name": "test.vocab/1", "path": "vocab/test.json", "hash": "b3:" + "0" * 64}
    ]
    named_refs_body["sections"]["features"] = [
        {"name": "test.feature/1", "path": "features/test.json", "hash": "b3:" + "0" * 64}
    ]
    body_validator.validate(named_refs_body)
    named_refs_body["sections"]["vocab"][0].pop("name")
    if body_validator.is_valid(named_refs_body):
        raise SystemExit("body schema accepted a vocabulary reference without its name")

    named_refs_body = read_json(FIXTURES / "cb9-minimal-void" / "body.json")
    named_refs_body["sections"]["vocab"] = [
        {"name": "test.vocab/1", "path": "vocab/test.json", "hash": "b3:" + "0" * 64}
    ]
    named_refs_body["sections"]["features"] = [
        {"name": "test.feature/1", "path": "features/test.json", "hash": "b3:" + "0" * 64}
    ]
    body_validator.validate(named_refs_body)
    named_refs_body["sections"]["vocab"][0].pop("name")
    if body_validator.is_valid(named_refs_body):
        raise SystemExit("body schema accepted a vocabulary reference without its name")

    capability_count = 0
    capability_ids = tomllib.loads((ROOT / "schema" / "capability_ids.toml").read_text(encoding="utf-8"))
    allocated = capability_ids["capabilities"]
    fixture_allocated = capability_ids.get("fixture_capabilities", {})
    all_allocated = [*allocated.values(), *fixture_allocated.values()]
    if len(set(all_allocated)) != len(all_allocated):
        raise SystemExit("capability IDs must be unique")
    declared_ids = set()
    capability_documents = []
    for path in sorted(CAPABILITIES.glob("*.json")):
        document = read_json(path)
        capability_validator.validate(document)
        Draft202012Validator.check_schema(document["params_schema"])
        capability_name = document["id"].removeprefix("veyra.cap.").removesuffix("/1")
        if capability_name not in allocated:
            raise SystemExit(f"capability {document['id']} has no permanent numeric allocation")
        if document["id"] in declared_ids:
            raise SystemExit(f"duplicate capability schema ID: {document['id']}")
        declared_ids.add(document["id"])
        local_ids = [template["local_id"] for template in document["field_templates"]]
        if any(not isinstance(local_id, int) or local_id < 1 or local_id > 65535 for local_id in local_ids):
            raise SystemExit(f"invalid local field ID in {document['id']}")
        if len(set(local_ids)) != len(local_ids):
            raise SystemExit(f"duplicate local field ID in {document['id']}")
        capability_documents.append(document)
        capability_count += 1
    if capability_count != 8:
        raise SystemExit(f"expected eight V1 capability documents, found {capability_count}")
    for document in capability_documents:
        missing = sorted(set(document["requires"]) - declared_ids)
        if missing:
            raise SystemExit(f"{document['id']} requires unknown capabilities: {', '.join(missing)}")

    compiled_contracts = {
        document["id"]: {
            "field_template_ids": [
                template["local_id"] for template in document["field_templates"]
            ],
            "requires": document["requires"],
            "params_schema": document["params_schema"],
            "required_reference_surface_kinds": document["required_reference_surface_kinds"],
        }
        for document in capability_documents
    }
    compiled_numeric_ids = {
        f"veyra.cap.{name}/1": numeric_id
        for name, numeric_id in {
            **allocated,
            **fixture_allocated,
        }.items()
    }
    compiled_contract_file = read_json(ROOT / "schema" / "capability_contracts.v1.json")
    if compiled_contract_file != {
        "schema": "veyra.capability_contracts/1",
        "numeric_ids": compiled_numeric_ids,
        "capabilities": compiled_contracts,
    }:
        raise SystemExit(
            "embedded capability contracts are stale; run py scripts/generate_capability_contracts.py"
        )

    fixture_count = 0
    for path in sorted(FIXTURES.rglob("body.json")):
        body = read_json(path)
        if path.parent.name == "major-plus-one":
            if body_validator.is_valid(body):
                raise SystemExit("body schema accepted the intentionally unsupported CB6 major version")
            fixture_count += 1
            continue
        body_validator.validate(body)
        registry_path = path.parent / body["sections"]["registry"]["path"]
        registry = read_json(registry_path)
        if path.parent.name == "unknown-critical-field":
            if registry_validator.is_valid(registry):
                raise SystemExit("field registry schema accepted an unknown critical semantic")
            if field_validator.is_valid(registry["fields"][0]):
                raise SystemExit("field descriptor schema accepted an unknown critical semantic")
            fixture_count += 1
            continue
        registry_validator.validate(registry)
        for descriptor in registry.get("fields", []):
            field_validator.validate(descriptor)
        fixture_count += 1

    print(
        f"PASS: {len(schemas)} schemas, {capability_count} capability documents, "
        f"{fixture_count} body fixtures validated"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
