#!/usr/bin/env python3
"""Compile the capability schema documents into the reader's embedded contract table."""

from __future__ import annotations

import json
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CAPABILITIES = ROOT / "schema" / "capabilities"
CAPABILITY_IDS = ROOT / "schema" / "capability_ids.toml"
OUTPUT = ROOT / "schema" / "capability_contracts.v1.json"


def main() -> int:
    allocations = tomllib.loads(CAPABILITY_IDS.read_text(encoding="utf-8"))
    numeric_ids = {
        f"veyra.cap.{name}/1": value
        for name, value in {
            **allocations["capabilities"],
            **allocations.get("fixture_capabilities", {}),
        }.items()
    }
    contracts = {}
    for path in sorted(CAPABILITIES.glob("*.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        contracts[document["id"]] = {
            "field_template_ids": [
                template["local_id"] for template in document["field_templates"]
            ],
            "requires": document["requires"],
            "params_schema": document["params_schema"],
            "required_reference_surface_kinds": document["required_reference_surface_kinds"],
        }
    projection = {
        "schema": "veyra.capability_contracts/1",
        "numeric_ids": numeric_ids,
        "capabilities": contracts,
    }
    with OUTPUT.open("w", encoding="utf-8", newline="\n") as stream:
        stream.write(json.dumps(projection, ensure_ascii=False, indent=2) + "\n")
    print(f"WROTE {OUTPUT.relative_to(ROOT)} from {len(contracts)} capability schemas")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
