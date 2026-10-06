#!/usr/bin/env python3
"""Compile the capability schema documents into the reader's embedded contract table."""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CAPABILITIES = ROOT / "schema" / "capabilities"
OUTPUT = ROOT / "schema" / "capability_contracts.v1.json"


def main() -> int:
    contracts = {}
    for path in sorted(CAPABILITIES.glob("*.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        contracts[document["id"]] = {
            "requires": document["requires"],
            "params_schema": document["params_schema"],
            "required_reference_surface_kinds": document["required_reference_surface_kinds"],
        }
    projection = {"schema": "veyra.capability_contracts/1", "capabilities": contracts}
    with OUTPUT.open("w", encoding="utf-8", newline="\n") as stream:
        stream.write(json.dumps(projection, ensure_ascii=False, indent=2) + "\n")
    print(f"WROTE {OUTPUT.relative_to(ROOT)} from {len(contracts)} capability schemas")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
