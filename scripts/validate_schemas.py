#!/usr/bin/env python3
"""Validate JSON schema documents, capability schemas, and committed body fixtures."""

from __future__ import annotations

import json
import tomllib
from copy import deepcopy
from pathlib import Path

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schema" / "json"
CAPABILITIES = ROOT / "schema" / "capabilities"
FIXTURES = ROOT / "conformance" / "worlds"


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    schemas = {path.name: read_json(path) for path in SCHEMAS.glob("*.json")}
    for schema in schemas.values():
        Draft202012Validator.check_schema(schema)
    capability_schema = schemas["capability_document.json"]
    body_schema = schemas["body.json"]
    field_schema = schemas["field_descriptor.json"]
    registry_schema = schemas["field_registry.json"]
    capability_validator = Draft202012Validator(capability_schema)
    body_validator = Draft202012Validator(body_schema)
    registry_validator = Draft202012Validator(registry_schema)
    field_validator = Draft202012Validator(field_schema)

    gm_fixture = read_json(FIXTURES / "cb9-minimal-void" / "body.json")
    for gm in ("1", "0.1", "1e3", "1.25e-3", "1e2147483647", "1e-2147483648"):
        gm_fixture["physical"]["gm_m3_s2"] = gm
        body_validator.validate(gm_fixture)
    for gm in (
        "0",
        "-1",
        "-0",
        "-0.0",
        "-0e3",
        "0.0",
        "0e3",
        "0.0e-3",
        "1e2147483648",
        "1e-2147483649",
        "+1",
    ):
        gm_fixture["physical"]["gm_m3_s2"] = gm
        if body_validator.is_valid(gm_fixture):
            raise SystemExit(f"body schema accepted invalid positive GM decimal: {gm}")

    frame_fixture = read_json(FIXTURES / "cb9-minimal-void" / "body.json")
    frame_mutations = []
    for frame in (None, 1, "body_fixed"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"] = frame
        frame_mutations.append(("frame shape", body))
    body = deepcopy(frame_fixture)
    body["frames"]["body_fixed"].pop("axes")
    frame_mutations.append(("missing axes", body))
    for axes in ("", "right-handed"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["axes"] = axes
        frame_mutations.append(("invalid axes", body))
    body = deepcopy(frame_fixture)
    body["frames"]["body_fixed"].pop("rotation")
    frame_mutations.append(("missing rotation", body))
    for key in ("kind", "period_s", "epoch", "orientation_q_at_epoch", "relative_to"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"].pop(key)
        frame_mutations.append((f"missing rotation {key}", body))
    for rotation in (None, 1, "uniform"):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"] = rotation
        frame_mutations.append(("rotation shape", body))
    for key, value in (
        ("kind", "precessing"),
        ("period_s", "not-a-period"),
        ("period_s", "0"),
        ("epoch", "00"),
        ("relative_to", "unknown_frame"),
    ):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"][key] = value
        frame_mutations.append((f"invalid rotation {key}", body))
    for quaternion in (["1", "0", "0"], ["1", "0", "NaN", "0"], ["1", "0", 0, "0"]):
        body = deepcopy(frame_fixture)
        body["frames"]["body_fixed"]["rotation"]["orientation_q_at_epoch"] = quaternion
        frame_mutations.append(("invalid orientation quaternion", body))
    body = deepcopy(frame_fixture)
    body["required_features"].append("veyra.topo.dir_cube/1")
    body["domains"] = [{
        "id": "surface", "topology": "veyra.topo.dir_cube/1", "frame": "missing_frame",
        "vertical": {"kind": "none"}, "tile_log2": 2, "max_level": 5,
    }]
    frame_mutations.append(("domain missing frame", body))
    for description, body in frame_mutations:
        if body_validator.is_valid(body):
            raise SystemExit(f"body schema accepted invalid {description}")

    radial_surface = deepcopy(frame_fixture)
    radial_surface["figure"] = {"kind": "radial_profile_sphere", "extent_m": "2"}
    radial_surface["reference_surfaces"] = [{"id": "figure.boundary", "kind": "figure_surface"}]
    if body_validator.is_valid(radial_surface):
        raise SystemExit("body schema accepted figure_surface for radial_profile_sphere")
    radial_photosphere = deepcopy(frame_fixture)
    radial_photosphere["figure"] = {"kind": "radial_profile_sphere", "extent_m": "2"}
    radial_photosphere["reference_surfaces"] = [{"id": "photosphere", "kind": "sphere", "radius_m": "2"}]
    body_validator.validate(radial_photosphere)
    for figure in (
        {"kind": "sphere", "radius_m": "2"},
        {"kind": "star_convex_radial", "radius_field": "figure.radius_m"},
    ):
        solid_figure = deepcopy(frame_fixture)
        solid_figure["figure"] = figure
        solid_figure["reference_surfaces"] = [{"id": "solid.boundary", "kind": "figure_surface"}]
        body_validator.validate(solid_figure)
    reserved_surface = deepcopy(frame_fixture)
    reserved_surface["reference_surfaces"] = [{"id": "reserved", "kind": "ellipsoid"}]
    if body_validator.is_valid(reserved_surface):
        raise SystemExit("body schema accepted a reserved reference-surface kind")

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
            "requires": document["requires"],
            "params_schema": document["params_schema"],
            "required_reference_surface_kinds": document["required_reference_surface_kinds"],
        }
        for document in capability_documents
    }
    compiled_contract_file = read_json(ROOT / "schema" / "capability_contracts.v1.json")
    if compiled_contract_file != {
        "schema": "veyra.capability_contracts/1",
        "capabilities": compiled_contracts,
    }:
        raise SystemExit(
            "embedded capability contracts are stale; run py scripts/generate_capability_contracts.py"
        )

    fixture_count = 0
    for path in sorted(FIXTURES.rglob("body.json")):
        body = read_json(path)
        body_validator.validate(body)
        registry_path = path.parent / body["sections"]["registry"]["path"]
        registry = read_json(registry_path)
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
