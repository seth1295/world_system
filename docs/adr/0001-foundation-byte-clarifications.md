# ADR 0001: Foundation byte-level clarifications

**Status:** accepted for the phase 01 implementation

## Context

The canonical architecture fixes the identity and artifact model, but leaves a few byte-level allocations and fixture conventions unstated. These choices make the foundational implementation reproducible without changing any frozen topology or authority rule.

## Decisions

- Birth-address tags are one byte. System and free-object addresses contain the three signed level-zero coordinates as minimal unsigned LEB128 after zigzag encoding, followed by a minimal unsigned LEB128 slot. Level zero is implicit in these address kinds and is not encoded. Body-in-system addresses contain the 16-byte system ID, one-byte role, and minimal unsigned LEB128 ordinal. Fixture addresses contain a minimal unsigned LEB128 UTF-8 byte length followed by the name bytes. Decoders reject overlong encodings, invalid UTF-8, trailing bytes, and nonzero address levels.
- The `uni:fixture` sentinel maps to the 32-byte BLAKE3 digest of the literal UTF-8 bytes `uni:fixture`. Fixture ObjectIds then use the same `veyra.object.v1` derive-key operation as other birth addresses.
- The VYB1 dtype byte allocation is `0` for non-raster raw content, followed by `1=u8`, `2=i8`, `3=u16`, `4=i16`, `5=u32`, `6=i32`, and `7=f32`. The index payload uses the raw dtype code.
- `shuffle2` divides canonical bytes into even-indexed and odd-indexed byte lanes before Zstandard compression. Decode restores the original order before checking the content hash. This codec operation never participates in blob identity.
- A new ledger starts at sequence zero with a 32-byte all-zero predecessor. Each subsequent entry's predecessor is the prior entry hash. The head of an empty ledger is the zero predecessor.
- Decimal strings use a base-ten mantissa with optional lowercase `e` exponent, no leading zeroes in the integer or exponent, and are retained as exact text for identity. Validation does not pass through binary float. An explicit `to_f64` conversion uses Rust's correctly rounded decimal parser for downstream numeric work and cannot alter stored text or hashes.
- Workspace crates are marked `publish = false` while the repository's license decision is pending. This allows dependency checks to cover third-party crate licenses without declaring a project license or adding a license file.

## Compatibility

These clarifications define the phase 01 writer and reader bytes. They do not alter the frozen ObjectId derivation formula, VYB1 header layout, content-addressing boundary, cube-face formulas, or radial key layout. The independent face-adjacency derivation continues to match all 24 committed table entries.
