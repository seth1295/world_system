# Schemas

Canonical JSON Schemas live in `json/`. The eight V1 physical capability documents live in `capabilities/`; their numeric field namespaces are allocated only in `capability_ids.toml`. That allocation file also has a separate fixture-only namespace for conformance probes.

The direction-cube face-edge table is `face_adjacency.toml`. `scripts/derive_adjacency.py` derives the mapping independently from the face formulas and checks the committed table.
