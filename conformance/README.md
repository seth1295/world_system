# Conformance corpus

`worlds/` contains committed, deterministic foundation artifacts. `vectors/canon/` contains byte-level identity/codec vectors and the published RFC 8785 Appendix B numeric samples. `vectors/topology/stage2-parity.json` pins the Stage 2 deterministic seed and native/WASM parity output digest. `acceptance/stages-00-03.json` maps Stage 0-3 acceptance requirements to executable source, tests, vectors, or CI evidence.

`veyra-conformance` builds and verifies the addressing, hashing, compatibility, and minimal-void fixtures. `scripts/compare_foundation_wasm.mjs` executes the small C ABI parity export under Node and compares its result bytes with the native payload. This harness is limited to foundation vectors and does not expose the later application API.
