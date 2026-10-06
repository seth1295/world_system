# Schemas

Canonical JSON Schemas live in `json/`. The eight V1 physical capability documents live in `capabilities/`; their numeric field namespaces are allocated only in `capability_ids.toml`. That allocation file also has a separate fixture-only namespace for conformance probes.

`capability_contracts.v1.json` is a generated embedded-reader projection of the capability documents' dependencies, parameter schemas, and reference-surface requirements. Regenerate it with `py scripts/generate_capability_contracts.py`; `py scripts/validate_schemas.py` verifies that it is synchronized with the source documents.

The direction-cube face-edge table is `face_adjacency.toml`. `scripts/derive_adjacency.py` derives the mapping independently from the face formulas and checks the committed table.
