#!/usr/bin/env python3
"""Validate JSON schema documents, capability schemas, and committed body fixtures."""

from __future__ import annotations

import json
import tomllib
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
