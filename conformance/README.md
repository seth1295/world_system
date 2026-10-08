# Conformance corpus

`worlds/` contains committed, deterministic body artifacts. `vectors/` contains byte-level vectors for identity, codecs, and topology. `queries/stage4.jsonl` and `expected/stage4.jsonl` pin the public sampler, periodic selection, weighted statistics, tiles, topology and capability-derived views, and geometry outputs; floating-point values are stored as exact IEEE-754 bit strings.

`veyra conformance gen` creates missing fixtures and expected output. `veyra conformance verify` opens each body through the writer/loader path, executes the committed queries through the core API, and compares each JSONL output byte-for-byte. `run_wasm_parity.mjs` defines the adapter contract for the later WASM runner and compares its returned JSONL bytes with the same expected file. Stage 4 verifies the `wasm32-unknown-unknown` build; executable native/WASM comparison remains in the Stage 10 bindings and harness work.

`docs/adr/0002-stage4-reduction-and-nodata-rules.md` records the current deterministic pyramid and nodata choices. It also identifies the unresolved `f32` pyramid conversion rule: V1 permits `f32` rasters while prescribing integer downsample arithmetic, so the current writer refuses to generate `f32` parent tiles.
