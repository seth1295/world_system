# Body and field schema parity

The JSON Schemas define the structurally expressible V1 contract. `BodyRoot` and
`BodyLoader` remain authoritative for identity derivation, cross-reference checks,
content verification and compatibility decisions that depend on other sections.

Run `py scripts/validate_schemas.py` to validate the published schemas, committed
fixtures, shared decimal rules and the named schema/core parity cases. The Rust
counterparts live in `veyra-core` body, registry, time and loader tests. The script
registers `schema/json/decimal.json` by its `$id`; schema consumers must resolve
that resource when validating `body.json` or `field_descriptor.json`.

## Intentional Rust-only checks

| Invariant | Why JSON Schema does not decide it | Rust coverage |
|---|---|---|
| `object_id` equals the ID derived from `origin` | Requires the VEYRA address codec and BLAKE3 derivation across fields. The schema checks both shapes. | `body_identity_derives_universe_birth_addresses`, `body_identity_must_derive_from_its_fixture_origin` |
| IDs are unique across registry fields, capabilities, domains and surfaces | Draft 2020-12 has `uniqueItems`, but no standard unique-by-property keyword. | `registry_field_ids_are_unique_before_ancillary_compatibility_is_applied`, `domain_schema_expressible_rules_and_rust_cross_checks_are_enforced`, `reference_surface_shapes_match_the_body_schema_and_rust_resolves_links` |
| Field allocation, domain membership, index/registry correspondence and star-radius field resolution | These compare registry entries, permanent allocation metadata, domains and indexes in separate parts of the artifact. | `validate_registry` tests, `figure_field_name_reference_must_resolve_unambiguously`, loader and writer contract tests |
| Domain topology appears in `required_features`; capability dependency closure and annotated parameter references | These depend on the topology set and capability contracts maintained in other files. | `domain_schema_expressible_rules_and_rust_cross_checks_are_enforced`, `known_capability_schemas_enforce_params_dependencies_and_references_generically` |
| Reference-surface bases resolve and offset graphs are acyclic | The schema can validate each `offset_of` object's shape, but not resolve names or detect cycles between array entries. | `reference_surface_shapes_match_the_body_schema_and_rust_resolves_links` |
| Section/index/blob hashes match actual artifact bytes and referenced content exists | Requires loading other files and hashing canonical or uncompressed bytes. | `BodyLoader`, conformance, and writer round-trip tests |
| A critical field's interpolation is supported by its domain topology | The field descriptor and domain are separate documents; the standalone field schema accepts the V1 interpolation union. | `critical_field_metadata_accepts_v1_contract_and_refuses_unknown_operators` |
| Body-fixed quaternion is unit length and finite in binary64 | Schema enforces four canonical decimal components. Norm and finite conversion are numeric semantic checks. | `body_fixed_frame_requires_a_complete_v1_uniform_rotation` |
| Duplicate JSON object names and lexical number canonicality | JSON Schema validates parsed values and cannot distinguish lexical forms such as integer `1` from `1.0`; the canonical parser rejects forbidden source forms and duplicate names. | JCS parser tests; the Python harness also uses a duplicate-aware JSON loader |

Unknown critical capability IDs remain a Rust check because the known set comes
from the capability contracts and allocation data. Critical field semantics are
constrained by `field_descriptor.json` and rechecked by Rust; ancillary unknown
semantics remain accepted and preserved. Dynamic fields are rejected at the body
registry schema boundary and by Rust even when marked ancillary.
