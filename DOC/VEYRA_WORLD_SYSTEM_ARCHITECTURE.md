# VEYRA World System: Canonical Architecture and Implementation Program

**Status:** canonical. This document replaces "VEYRA World Core V1: implementation specification", which treated an Earth-like planet as the root ontology. That document is void.
**Repository:** https://github.com/seth1295/world_system (public, `main`)
**Local root:** `D:\PORT_WORK\Experiment_world_system`
**Save as:** `docs/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md`

**Conventions.** MUST / MUST NOT / SHOULD are normative. **FROZEN** means that changing it requires a new format major version. **(V1)** means built in V1. **(reserved)** means the schema and architecture guarantee it is not blocked, but no V1 code implements it. Sizes and levels given for test profiles are test parameters, not format definitions.

**One-paragraph summary.** VEYRA is a persistent procedural universe. Every celestial object has a stable deterministic identity before it exists physically. Objects become more resolved as they become relevant. Only when a body is materialized does it get a **Body Artifact** (`.veyra`): a sparse, content-addressed, schema'd store of fields, features and state, described by declared **capabilities** rather than by terrestrial assumptions. After materialization the artifact is the permanent authority for that physical object. One Rust core owns all semantics and is compiled to native, WASM and a C ABI. Browsers, Unreal and tools are consumers that never invent canonical state.

---

## 1. Product model

```
PROCEDURAL UNIVERSE      (universe seed + versioned recipe contract; infinite, almost entirely unmaterialized)
        │ stable addresses → stable object identities
        ▼
POTENTIAL OBJECTS        (systems, bodies, free objects: defined by recipe, zero persistent storage)
        │ relevance raises resolution: catalogue → system → orbit
        ▼
MATERIALIZATION          (macro body generation; atomic commit; recipe stops being authority)
        ▼
PERSISTENT BODIES        (.veyra artifacts: baseline + edits + sim state + dynamics ledger)
        │ deeper relevance: local refinement / sealed extensions
        ▼
LOCAL SIMULATION / GAMEPLAY   (consumers: Inspector now; Unreal and servers later)
```

Principles:

1. **PCG resolution follows relevance.** A telescope observation never generates caves.
2. **Identity precedes existence.** An object has a permanent ID before anything about it is stored.
3. **Materialization is a one-way authority transfer** from recipe to artifact.
4. **Universal architecture, narrow first implementation.** V1 builds one rich terrestrial-style testcase, one stellar-structure fixture and one irregular airless fixture. Nothing in the core is terrestrial.
5. **Consumers are not authorities.** The Inspector, Unreal and servers read canonical state through the core. They never create it.

---

## 2. Terminology (FROZEN vocabulary)

| Term | Definition |
|---|---|
| **Universe** | A persistent procedural creation: seed, recipe contract, time, registry of what has been resolved and materialized, and global state. Identified by `UniverseId`. |
| **Recipe contract** | The set of versioned, immutable generation algorithms (`veyra.ugen.*`) pinned in the universe. |
| **Region** | A cell of the universe's integer spatial lattice, used for generation seeding and spatial indexing. |
| **System** | A gravitationally organised group of bodies (or a lone star). Has a permanent `ObjectId` and a birth manifest. It does not own its bodies. |
| **Celestial Body** | Any physically meaningful object: terrestrial planet, moon, asteroid, comet, dwarf planet, gas or ice giant, star, interstellar object. The core ontology is "body", never "planet". |
| **Object** | Anything with an `ObjectId`: systems and bodies. |
| **Object Address** | The canonical, recipe-defined birth address of an object. Immutable. |
| **ObjectId** | 128-bit digest of (UniverseId, Object Address). The permanent identity of the object, before and after materialization. |
| **Sketch** | A deterministic, non-persistent, recipe-derived summary of a potential body (class, mass, rough size, orbit). |
| **Materialization** | Generating a body's macro baseline and committing it as persistent authority. |
| **Body Artifact (`.veyra`)** | One logical complete materialized body: baseline + state layers + content. |
| **Body Baseline** | The immutable, content-addressed generated definition of the body. Its hash is the `BaselineId`. |
| **Sealed Extension** | An immutable, content-addressed addition to a baseline, created by later deeper materialization. |
| **Capability** | A versioned schema module declaring that a body has a physical domain (`solid_surface`, `stellar_structure`, `nucleus_shape`, …), with its fields, features, vocabularies, parameters, derived views and explain recipes. |
| **Body class** | A named, optional composition profile of capabilities (`veyra.class.*`) used for validation and generator selection. It is not a type system. |
| **Domain** | A spatial space inside a body in which fields live: topology + reference frame + vertical definition + extent. |
| **Topology** | A spatial addressing scheme implemented by the core (`veyra.topo.*`). |
| **Field** | A schema'd, sampled quantity on a domain, stored as tiles. |
| **Feature** | A discrete entity with a stable ID in a typed table (plates, rivers, vents, …), linked to cells by `feature_ref` fields or covers. |
| **Reference Frame** | A coordinate frame with a defined relation to its parent frame. |
| **Reference Figure** | The body's declared shape model. |
| **Reference Surface** | A named, declared surface (datum, ocean level, photosphere, isobar) used to define heights, depths and classifications. |
| **Refinement** | The normative, deterministic definition of canonical detail below the finest stored level. |
| **Edit Layer** | An append-only record of persistent physical modifications to a body. |
| **Simulation State** | The mutable, evolving state of a body's `dynamic` fields and processes. |
| **Dynamics Ledger** | The append-only record of a body's dynamical keyframes and events. |
| **Save / Checkpoint** | A manifest binding a set of ledger heads and snapshots at one universe time. |
| **Cache** | Reconstructable derived data. Never part of any identity. |
| **Consumer** | Anything that reads canonical data: Inspector, validator, Unreal adapter, server. |

---

## 3. Authority model

The **single-truth rule**: every fact has exactly one owner. Any other place that holds it must hold either a hash reference to the owner or a **derived** copy that is labelled as derived, rebuildable, and checked by the validator.

| Authority | Owns | Does NOT own |
|---|---|---|
| **Universe recipe** (pre-materialization) | Whether an unmaterialized object exists, its class, sketch, birth orbit, and analytic trajectory (as procedural truth). | Anything about a materialized body's physical content. |
| **Universe state** | Global time; object status (lifecycle); system membership relation ledger; the materialization registry; spatial index (derived); player and session state; save manifests. | A body's physical fields; a materialized body's dynamical state vectors. |
| **Body baseline** | What the body physically is at materialization: identity, classification, physical parameters (GM), figure, frames, reference surfaces, capabilities, fields, features, vocabularies, provenance, the dynamics descriptor and origin keyframe. | Anything that changes after materialization. |
| **Body edit layer** | Persistent physical modifications. | Baseline content (never rewritten). |
| **Body simulation state** | The evolving state of `dynamic` fields and internal processes. | Dynamics (orbit). |
| **Body dynamics ledger** | The current dynamical truth (keyframes and events). Storage is in the body; the **sole writer** is the universe dynamics service. | Membership; global time. |
| **Caches** | Nothing. | Any truth. |
| **Consumer engines** | Presentation (C1, C2). Runtime actor and physics state, kept in separate gameplay schemas. | Canonical physical state. |
| **Inspector** | Nothing. It displays what the core returns. | Semantics. |

**Rules**

- A materialized body's baseline is **frozen forever**. A newer generator never regenerates a materialized body. A better generator makes new bodies, or a new universe. Explicit migration is a separate, lineage-recording tool.
- The Inspector, Unreal and servers MUST NOT compute cell IDs, projections, interpolation, derived layers, refinement, feature resolution, orbital propagation or class membership. They call the core.
- C0 (canonical) data is produced only by the core. Consumers may add C1 and C2 presentation only (§13).

---

## 4. Universal body model

### 4.1 Layered ontology

```
1. Universal body core        identity, classification, GM, figure, frames, reference surfaces, dynamics descriptor
2. Spatial infrastructure     topologies, domains, cell addressing, tiling, adjacency
3. Capability schemas         domain modules that declare fields, features, vocabularies, derived views, explain recipes
4. Body-class compositions    optional named capability sets used for generator selection and validation
```

A body MAY have zero capabilities and still be valid (`cb9-minimal-void`). Nothing in layers 1 and 2 mentions terrain, sea, soil, seasons, or hemispheres.

### 4.2 Body root (`body.json`) schema (V1)

```json
{
  "schema": "veyra.body/1",
  "format_version": {"major": 1, "minor": 0},
  "required_features": ["veyra.topo.dir_cube/1", "veyra.refine.cdetail/1", "veyra.codec.zstd-shuffle2/1",
                        "veyra.cap.topography/1", "veyra.cap.tectonics/1"],
  "identity": {
    "object_id": "obj:9f3c…(32 hex)",
    "origin": {"kind": "universe", "universe_id": "uni:b3:…", "address": {"kind": "body_in_system", "system": "obj:…", "role": 2, "ordinal": 3}},
    "label": "Veyra"
  },
  "classification": {"class": "veyra.class.terrestrial.habitable_test/1", "tags": ["rocky", "atmosphere", "ocean"]},
  "physical": {"gm_m3_s2": "398600441800000", "gravity_model": "point_mass"},
  "figure": {"kind": "sphere", "radius_m": "6000000"},
  "frames": {
    "body_fixed": {"axes": "+Z is the positive rotation pole; +X is the prime meridian; right-handed",
                   "rotation": {"kind": "uniform", "period_s": "86400", "epoch": "0",
                                "orientation_q_at_epoch": ["1", "0", "0", "0"],
                                "relative_to": "universe_inertial"}}
  },
  "reference_surfaces": [
    {"id": "datum.mean", "kind": "sphere", "radius_m": "6000000"},
    {"id": "ocean.level", "kind": "offset_of", "base": "datum.mean", "offset_m": "0"}
  ],
  "dynamics": {"descriptor": {"path": "dynamics/descriptor.json", "hash": "b3:…"},
               "origin_keyframe": {"path": "dynamics/origin.json", "hash": "b3:…"}},
  "capabilities": [
    {"id": "veyra.cap.solid_surface/1", "params": {"figure_ref": "figure"}},
    {"id": "veyra.cap.topography/1",    "params": {"reference_surface": "datum.mean", "domain": "surface"}},
    {"id": "veyra.cap.tectonics/1",     "params": {"domain": "surface"}},
    {"id": "veyra.cap.ocean/1",         "params": {"level_surface": "ocean.level", "domain": "surface"}},
    {"id": "veyra.cap.climate/1",       "params": {"domain": "surface", "slices": {"count": 12, "period_ref": "dynamics.orbital_period", "origin_ref": "dynamics.periapsis"}}}
  ],
  "domains": [{"id": "surface", "topology": "veyra.topo.dir_cube/1", "frame": "body_fixed",
               "vertical": {"kind": "none"}, "tile_log2": 7, "max_level": 30}],
  "codec": "zstd+shuffle2",
  "sections": {
    "registry": {"path": "registry/fields.json", "hash": "b3:…"},
    "vocab": [{"name": "tectonics.crust_type/1", "path": "vocab/…", "hash": "b3:…"}],
    "features": [{"name": "tectonics.plate", "path": "features/…", "hash": "b3:…"}],
    "provenance": {"dag": {"path": "provenance/dag.json", "hash": "b3:…"}, "explain": {"path": "provenance/explain.json", "hash": "b3:…"}}
  },
  "indexes": {"0x01010001": "b3:…"},
  "extensions_ledger": null
}
```

Rules:

- The root contains **no** field named elevation, sea, soil or season. Those exist only inside capability parameters and field descriptors.
- `physical.gm_m3_s2` is authoritative. Mass is derived using the universe's pinned `G`, or declared standalone for fixtures. One truth.
- `figure.kind` ∈ `sphere` (V1) · `star_convex_radial` (V1; radius from a stored field) · `ellipsoid` (reserved) · `radial_profile_sphere` (V1; spherical star or giant with no solid boundary; extent only) · `mesh` / `implicit` (reserved).
- Standalone fixtures use `origin: {"kind": "fixture", "name": "..."}` and an ObjectId derived from the sentinel universe `uni:fixture`.
- Unknown capability IDs marked `critical` (the default) make the reader refuse. A body may embed a capability schema (`x-` namespace) for forward-compatible experiments.

### 4.3 Capability schema

A capability is a versioned JSON schema document `veyra.cap.<name>/<n>` in `schema/capabilities/`, containing:

```
id, version, compat(critical|ancillary), requires:[capability ids],
params_schema,
field_templates:[ {local_id, name, semantic, storage, unit, domain_kind, temporal, sampling, downsample, refinement?, display, provenance} ],
feature_tables:[...], vocab:[...],
derived_views:[ {id, label, needs:[fields], op: core builtin id} ],
explain_recipes:[...], required_reference_surface_kinds:[...],
display: {group, label, order}
```

`FieldId = (capability_id: u16) << 16 | local_id: u16`. Capability numeric IDs are allocated **only** in `schema/capability_ids.toml`, are never reused, and carry no meaning in their numeric ranges. Ranges `0xE000+` are reserved for core-built derived views and `0xF000+` for diagnostics.

**V1 capability schemas** (implemented):

| Capability | Purpose in V1 | Fixtures that use it |
|---|---|---|
| `solid_surface` | Declares a solid boundary defined by the figure. | terrestrial, rock |
| `topography` | Height above a **named reference surface**; roughness; height components; dominant process. | terrestrial |
| `tectonics` | Plates, boundaries, crust type and age. | terrestrial |
| `ocean` | Declares a liquid level surface; derived land/ocean view. | terrestrial |
| `climate` | Periodic temperature and precipitation. | terrestrial |
| `surface_material` | Categorical surface material. | rock |
| `thermal_state` | Surface temperature. | rock |
| `stellar_structure` | Radial profile of density, temperature, pressure and composition. | star |

Reserved and not implemented in V1: geology, hydrology, atmosphere, cryosphere, soils, ecology, subsurface, caves, resources, impact_geology, regolith, cloud_layers, circulation, chemistry, magnetic_field, interior_layers, radiative_zone, convective_zone, photosphere, corona, stellar_activity, stellar_wind, spectral_output, nucleus_shape, volatile_species, jets, coma, tail, and more. Each is just another schema.

### 4.4 Body classes

`veyra.class.*` is a named, optional set `{required capabilities, recommended capabilities, generator id}`. The validator may check that a body that declares a class satisfies it. Nothing in the reader switches on class. Only the **universe materializer** selects a generator by class.

---

## 5. Body class examples (illustrative; only the first three are V1)

| Class | Capabilities | Domains and topologies | V1 |
|---|---|---|---|
| `terrestrial.habitable_test/1` | solid_surface, topography, tectonics, ocean, climate | `surface`: dir_cube | **Rich testcase** |
| `stellar.main_sequence_lite/1` | stellar_structure | `interior`: radial_1d; figure `radial_profile_sphere`; reference surface `photosphere` | **Fixture and universe body (Auren)** |
| `rocky.airless_irregular_lite/1` | solid_surface, surface_material, thermal_state | `surface`: dir_cube; figure `star_convex_radial` | **Fixture** |
| `rocky.airless_regular` (dead moon) | solid_surface, crust, impact_geology, regolith, thermal_state, subsurface | dir_cube, later layered | reserved |
| `small.comet` | nucleus_shape, composition, porosity, volatile_species, dust, thermal_state, fractures, active_regions, jets, sublimation, coma, tail, solar_heating, orbital_history | star-convex or mesh nucleus; volumetric coma fields | reserved |
| `giant.gas` / `giant.ice` | gravity, atmosphere, cloud_layers, circulation, chemistry, magnetic_field, interior_layers | radial_1d plus dir_cube layered or volumetric | reserved |
| `stellar.*` (full) | stellar_structure, composition, radiative_zone, convective_zone, photosphere, chromosphere, corona, magnetic_field, stellar_activity, stellar_wind, spectral_output | radial_1d plus volumetric | reserved |
| `interstellar.object` | like comet or asteroid, with unbound dynamics | | reserved |

The Inspector shows a group only if the body declares the capability. A body without ecology has no Ecology group.

---

## 6. Universe model

### 6.1 Universe root

`universe.json` is **immutable** and defines identity:

```json
{
  "schema": "veyra.universe/1",
  "format_version": {"major": 1, "minor": 0},
  "required_features": ["veyra.ugen.minimal/1", "veyra.uidx.lattice/1"],
  "seed": "0x…u64 hex",
  "recipe_contract": {"ugen": "veyra.ugen.minimal/1", "body_generators": {"terrestrial.habitable_test/1": "veyra.gen.terrestrial_test/1", "stellar.main_sequence_lite/1": "veyra.gen.star_lite/1"}},
  "spatial_index": {"scheme": "veyra.uidx.lattice/1", "seeding_cell_edge_m": "9460730472580800", "origin": [0, 0, 0]},
  "time": {"unit": "ns", "epoch_zero": "universe_epoch", "ticks_type": "i128"},
  "constants": {"G_m3_kg_s2": "6.67430e-11"},
  "label": "TestUniverse-001"
}
```

`UniverseId = b3(JCS(universe.json))`. Mutable state is never inside `universe.json`.

### 6.2 Spatial index (`veyra.uidx.lattice/1`, V1)

- A universe-fixed, right-handed, integer lattice. A **seeding cell** is `(ix, iy, iz): i64`; its edge is `seeding_cell_edge_m` (exact integer metres). Coarser aggregate levels exist as octree parents: level `k` has edge `edge · 2^k`.
- A **RegionKey** is `(level: u8, ix, iy, iz)`. Generation happens only at seeding level 0.
- All universe-scale positions are **fixed-point integers**: position `i128` micrometres relative to a declared reference (§14). No f64 universal positions.
- Galaxy structure (density, arms) is **not designed now**. `ugen` contracts decide how many systems and free objects a seeding cell yields. The index imposes no structure.

### 6.3 Stable object identity

```
ObjectAddress kinds (canonical binary encoding, LEB128, zigzag for signed):
  1 system_seed    : level0 region (ix,iy,iz), slot u32
  2 body_in_system : system ObjectId (16 bytes), role u8, ordinal u32
  3 free_object    : level0 region (ix,iy,iz), slot u32        (interstellar objects born in a cell)
  4 fixture        : utf8 name

ObjectId = blake3.derive_key("veyra.object.v1", UniverseId_bytes(32) ‖ addr_bytes)[0..16]
text form: obj:<32 lowercase hex>
```

- The address is the **birth address**. It is never rewritten, even when the object changes system, is ejected, or is captured. A comet ejected from system S gets a `body_in_system(S, role=comet, n)` address and keeps it forever.
- Catalogue and materialization records store the **address**. The digest alone cannot be inverted back to recipe inputs.
- `ObjectId` is independent of the generator build. It depends on `UniverseId`, so a new recipe contract yields a new universe and new IDs.
- A system is an object. A star is a body in a system with `role = star`.

### 6.4 Identity hierarchy

| Level | ID | Derived from | Stable across |
|---|---|---|---|
| Universe | `uni:b3:<64hex>` | JCS of `universe.json` | forever |
| Region | `RegionKey` | lattice coordinates | forever |
| System | `obj:<32hex>` | UniverseId + system_seed address | lifecycle, saves |
| Body | `obj:<32hex>` | UniverseId + body address | catalogue, materialization, edits, system changes, ejection |
| Feature | `(ObjectId, table, stable_id u64)` | `blake3.derive_key("veyra.feat.v1", ObjectId ‖ table ‖ structural_key)[..8]` | within the baseline |
| Field | `FieldId u32` | capability ID and local ID | forever, never reused |
| Chunk | `(FieldId, level, tile key)` | topology | immutable per baseline |
| Baseline | `bas:b3:<64hex>` | hash of body root | immutable |
| Extension | `ext:b3:<64hex>` | hash of extension manifest | immutable |
| Edit head | `edt:b3:<64hex>` | edit ledger head hash | per state |
| Sim snapshot | `sim:b3:<64hex>` | snapshot manifest | per state |
| Dynamics head | `dyn:b3:<64hex>` | dynamics ledger head | per state |
| Body state | `bst:b3:<64hex>` | JCS of `{baseline, extensions, edit head, sim, dyn head}` | one per checkpoint |
| Save | `sav:b3:<64hex>` | JCS of save manifest | one per checkpoint |

---

## 7. Materialization lifecycle

Seven stages. Each says what is **persisted**, what is merely **derived**, and where authority sits.

| # | Stage | What exists | Persisted | Authority |
|---|---|---|---|---|
| M0 | **Unseen** | The object as pure potential. | Nothing. The ID is computable from the address on demand. | Recipe |
| M1 | **Catalogued** | The player or simulation has learned that it exists (observed, scanned, or named in a chart). | Registry ledger record `{object_id, address, status: catalogued, t}`. The catalogue summary is derived from the recipe. | Recipe |
| M2 | **System resolved** | The system's birth manifest (member object IDs and roles) is known. | System record `{system_id, born_members: [object IDs and addresses], recipe_version}` plus a registry record. | Recipe |
| M3 | **Orbital resolved** | Bodies have sketches: class, GM, rough size, birth orbit. Visible in navigation, ephemeris and telescopes. | Registry status `orbital_resolved`. Sketches are recomputed from the recipe on demand and cached (cache only). | Recipe (analytic trajectory) |
| **M4** | **Body macro materialized** | **The body artifact first exists.** Macro fields, features, composition, figure, frames, dynamics descriptor and origin keyframe. | The full baseline, committed atomically. Registry record `{materialized, baseline_id}`. | **Body** |
| M5 | **Local materialized** | Deeper detail for a region of interest. | Normally nothing (pure normative refinement). Only generators that cannot be pure functions of the baseline write a **sealed extension**, listed in the extension ledger. | Body (once sealed) |
| M6 | **Persistent / modified** | The body has edits, sim state or dynamics events. | Edit ledger, sim snapshots, dynamics ledger. | Body |

**Rules**

- Transitions M0→M1→M2→M3 write only small registry records. A recipe may be re-evaluated; the determinism rule makes it reproduce the same sketch within one universe. Recipe contract versions are immutable (§17.3).
- **Constraint rule:** the materializer receives the sketch as hard constraints. The baseline records `origin.sketch_hash`, and the validator checks that the baseline's GM, class, and birth orbit match the sketch.
- **Commit protocol (M3→M4):**
  1. generate into `materialized/.staging/<tmp>/`;
  2. run `validate --level standard`;
  3. compute `BaselineId`;
  4. atomic rename into `materialized/<object_id>.veyra/`;
  5. append the registry ledger record `materialized` (the **commit point**).
  A directory without a ledger record is an orphan and may be adopted only by `veyra universe repair` after verification. A ledger record without its directory is a corruption error.
- **Persistence boundary:** persistent body authority begins exactly at the ledger record in step 5.
- Reopening a materialized body MUST load the artifact. No generator is invoked. A test proves it (§21).
- M5 default is **normative refinement** (pure function of the baseline plus the pinned algorithm). A sealed extension is allowed only when the algorithm is non-local or expensive. It is content-addressed and recorded in the extension ledger, and after sealing it is as authoritative as the baseline.

---

## 8. File / artifact hierarchy (development form)

### 8.1 Universe directory

```
TestUniverse-001.veyra-universe/
├─ universe.json                     # immutable recipe/identity → UniverseId
├─ universe.id                       # convenience copy, verified
├─ universe.meta.json                # NOT hashed: tool build, creation time, host
├─ generation/
│  └─ contract.json                  # pinned recipe contract details/test vectors reference
├─ registry/
│  ├─ objects.ledger.jsonl           # hash-chained status transitions (M1..M6)
│  ├─ systems/<system_obj>.json      # M2 birth manifests (immutable once written)
│  ├─ relations.ledger.jsonl         # hash-chained membership events (joined/left/captured/ejected)
│  └─ index/                         # DERIVED spatial/status index, rebuildable
├─ materialized/
│  ├─ obj_<hex>.veyra/               # body artifact directories (flat, by ObjectId; NOT under systems)
│  └─ .staging/
├─ store/
│  └─ blobs/ab/abcdef….zst           # shared content store (optional; bodies may keep local blobs)
├─ state/
│  ├─ time.json                      # current universe time (mutable, atomically replaced)
│  ├─ dynamics_index/                # DERIVED
│  ├─ players/  sessions/
│  └─ saves/<sav_hex>.save.json
└─ cache/                            # sketches, resolved summaries; deletable
```

Systems appear in `registry/systems/`, not as folders containing bodies. A body artifact is not "a file belonging to a system".

### 8.2 Body artifact (development form)

```
obj_9f3c….veyra/
├─ body.json                         # root manifest: Merkle root → BaselineId
├─ body.id                           # "bas:b3:…" convenience copy, verified
├─ body.meta.json                    # NOT hashed: generator build, times, host
├─ registry/fields.json  vocab/*.json  features/*.json  provenance/{dag,explain}.json
├─ dynamics/{descriptor.json, origin.json}                # baseline (hashed)
├─ index/<fieldhex>.idx
├─ blobs/ab/abcd….zst                # body-local blobs (any hash may instead resolve in universe store/)
├─ extensions/
│  └─ ledger.jsonl                   # sealed extension ledger (append-only, hash-chained; reserved)
├─ state/                            # mutable layers; never part of BaselineId
│  ├─ edits/{ledger.jsonl, blobs}    # reserved
│  ├─ sim/{journal.jsonl, snapshots} # reserved
│  └─ dynamics/ledger.jsonl          # keyframes/events (head present from M4: genesis entry references origin keyframe)
├─ diagnostics/                      # NOT hashed
│  ├─ diag.json
│  └─ blobs/
└─ cache/                            # NOT hashed
```

### 8.3 Four forms of the same logical body

| Form | Meaning | Contents |
|---|---|---|
| **Logical complete body** | What a `.veyra` conceptually is: "this is that physical object". | Baseline + sealed extensions + state layers + the closure of all referenced content. |
| **Development storage** | The directory above. | Manifests + blobs that may resolve in a shared store. |
| **Runtime shared content store** | `store/blobs/` at universe level, deduplicating bodies against each other. | Blobs only. |
| **Packed / exported body** | Self-contained archive of the **closure** at a chosen checkpoint. | Everything needed to open the body with no universe present. |

`BlobSource` resolution order: body-local `blobs/` → universe `store/` → error. Every blob is hash-verified on load, so location never matters.
**Export** (V1, directory form): `veyra body export` copies the closure into a standalone directory. The test proves it opens on a machine with no universe and no store. A single-file packed `.veyra` is the same closure serialized (§22), with no hash changes.

---

## 9. Content addressing and identity

- **Hash:** BLAKE3-256, text form `b3:<64 lowercase hex>`. The prefix allows algorithm change.
- **JSON:** UTF-8, **RFC 8785 JCS**. Hashed JSON MUST NOT contain non-integer JSON numbers. Quantities are decimal strings. Integers above 2^53−1 are strings. `section hash = b3(JCS(parse(file)))`, so pretty-printing never affects identity.
- **Canonical blob** = 16-byte header + payload, little-endian everywhere:
  `0..3 "VYB1" · 4 kind (1 raster tile, 2 index, 3 columnar reserved) · 5 dtype · 6 flags=0 · 7 =0 · 8..9 dim_i u16 · 10..11 dim_j u16 · 12..13 slices u16 · 14..15 =0`; payload `[slice][j][i]`, i fastest. For 1D topologies `dim_j = 1`.
  `blob_id = b3(canonical bytes)`.
- **Codec is not identity.** On disk: `zstd(shuffle(canonical))`, with the codec declared in `body.json`. The loader verifies `b3(decompress(file)) == blob_id` always, including in the browser.
- **Index blob** (kind 2): header, then `{field_id u32, entry_count u32, topology tag u8, tile_log2 u8, key_bytes u8 (=8 in V1), pad}`, then entries sorted by `(level u8, key u64)`: `level u8 | flags u8 (bit0=const) | pad 6 | key u64 | 32 bytes (hash, or const: i64 LE raw in the first 8 bytes)`.
- **Constants:** a tile whose every value is identical (including all-nodata) is a `const` entry with no blob. Sparse stays sparse.
- **Dedup:** automatic. Writers MUST NOT write an existing blob.
- **BaselineId** = `b3(JCS(body.json))`. `body.json` is a Merkle root through section and index hashes. It includes `identity.object_id`, the origin, and the dynamics origin keyframe hash. Two bodies cannot share a BaselineId.
- **Excluded from identity by construction:** `*.meta.json`, `diagnostics/`, `cache/`, `state/`, timestamps, hosts, paths, generator build IDs.
- **Ledgers** (registry, relations, extensions, edits, sim, dynamics): JSONL, each line `{seq, prev: "b3:…", type, payload, t}`; `entry_hash = b3(JCS(line_without_hash))`; the **head** is the last entry hash. Append-only. A ledger with a broken chain fails verification.
- **Versions:** `format_version {major, minor}` per format (`veyra.body`, `veyra.universe`). A reader MUST refuse if major differs or any `required_features` entry is unknown.

---

## 10. Spatial architecture

### 10.1 The abstraction

A field lives in a **Domain**:

```
Domain = Topology × Frame × Vertical × Extent
```

- **Topology** (`veyra.topo.*`): discretizes a *chart* of space into hierarchical cells with integer keys.
- **Frame**: the body-fixed frame (or another declared frame) that relates domain coordinates to physical space.
- **Vertical** defines what the non-topological dimension means: `none` · `radius` · `height_above(surface)` · `depth_below(surface)` · `pressure` · `optical_depth` (the last four reserved).
- **Extent**: the physical range the domain covers (radial extent in metres, or surface only).

Core trait (conceptual; Rust trait `Topology`):

```
id() -> TopologyId
locate(local_point) -> Option<CellKey>                  // exact, integer-based where defined
cell_center(key) -> LocalPoint        cell_measure(key) -> f64   (solid angle or volume weight)
parent(key) / children(key) -> [CellKey]       level(key)
neighbors(key, stencil) -> [Option<(CellKey, Transform)>]   // handles discontinuities such as cube-face edges
tile_key(key, tile_log2) -> TileKey        tile_cells(tile_key) -> layout
interp_stencil(point, level, kind) -> weights over keys
pyramid_children(key)   // the downsample relation
```

Everything above this trait (indexes, sampler, pyramid, refinement hooks, stats, views, validation, inspector) is topology-agnostic. A new topology is a new implementation plus a new ID in `required_features`. **No other part of the system changes.**

| Topology | Chart | V1? | Used for |
|---|---|---|---|
| `veyra.topo.dir_cube/1` | Unit directions S² on the cube-sphere (cell-centred, S2-style) | **V1** | Surface-like fields on spherical or star-convex bodies; the surface of the terrestrial testcase and the rock fixture |
| `veyra.topo.radial_1d/1` | Normalized radius in [0,1] of the domain extent | **V1** | Spherically symmetric interior or envelope profiles (star fixture) |
| `veyra.topo.dir_cube_layered/1` | dir_cube × vertical layers | reserved | Atmospheres, oceans, soil columns, subsurface |
| `veyra.topo.octree_vol/1` | Sparse 3D octree in a body frame | reserved | Volumetric stars and giants, comae, caves |
| `veyra.topo.mesh/1` | Unstructured surface or volume mesh | reserved | Concave or contact-binary bodies, nucleus meshes |
| `veyra.topo.points/1` | Point sets | reserved | Sparse measurements |

### 10.2 V1 topology `veyra.topo.dir_cube/1` (FROZEN)

**Frame.** Body-fixed, right-handed, metres. +Z is the positive rotation pole, +X the prime meridian direction, +Y completes the frame. A direction is a unit vector `(x,y,z)`. Axial latitude and longitude are a **display chart** only: `x=cosφ cosλ, y=cosφ sinλ, z=sinφ`. They exist for any body that declares a rotation, and they are not "north/south".

**Faces** (S2 numbering) and point→face:

| face | direction `(x,y,z)` for face coords `(u,v)∈[−1,1]²` | u from point | v from point |
|---|---|---|---|
| 0 | (1, u, v) | y/x | z/x |
| 1 | (−u, 1, v) | −x/y | z/y |
| 2 | (−u, −v, 1) | −x/z | −y/z |
| 3 | (−1, −v, −u) | z/x | y/x |
| 4 | (v, −1, −u) | z/y | −x/y |
| 5 | (v, u, −1) | −y/z | −x/z |

Face = axis of max |component|; ties go to the lowest axis index X<Y<Z, and sign selects the face.

**Warp** `uv→st∈[0,1]` (S2 quadratic): `s = 0.5·sqrt(1+3u)` for `u≥0`, else `1 − 0.5·sqrt(1−3u)`. Inverse: `u=(4s²−1)/3` for `s≥0.5`, else `u=(1−4(1−s)²)/3`. Only `sqrt` and basic arithmetic: bit-exact.

**Cells.** Level `L` has `N=2^L` cells per face edge, `L∈[0,30]`. `i=min(N−1,⌊s·N⌋)` (follows u/s), `j=min(N−1,⌊t·N⌋)` (follows v/t). **Cell-centred:** sample points are centres `s=(i+0.5)/N`. No shared vertices across face edges, so there is no seam-ownership question.

**CellKey (u64).** `key = (face<<61) | (P<<(61−2L)) | (1<<(60−2L))`, where `P` is the 2L-bit Morton path (at step `k`, `q_k = (bit_{L−k}(i)<<1)|bit_{L−k}(j)`). Level = `(60 − trailing_zeros(key))/2`. A prefix is a subtree and numeric order gives locality.

**Tiles.** Tile edge `E=2^min(T,L)` where `T=tile_log2` is a body constant (production 7, conformance 3). A tile key is `(L, ancestor key at level max(0, L−T))`. **Halo:** none stored. The core assembles halos on request across faces via the adjacency table.

**Adjacency** `(face, edge) → (face', edge', flip)`. `U±` are the i-edges and `V±` the j-edges. The along-edge coordinate is `a=j` for U edges and `a=i` for V edges. `flip` means `a'=N−1−a`. The cross index is `0` on `−` edges and `N−1` on `+` edges.

```
F0: U+→F1.U− same | U−→F4.V+ flip | V+→F2.U− flip | V−→F5.V+ same
F1: U+→F3.V− flip | U−→F0.U+ same | V+→F2.V− same | V−→F5.U+ flip
F2: U+→F3.U− same | U−→F0.V+ flip | V+→F4.U− flip | V−→F1.V+ same
F3: U+→F5.V− flip | U−→F2.U+ same | V+→F4.V− same | V−→F1.U+ flip
F4: U+→F5.U− same | U−→F2.V+ flip | V+→F0.U− flip | V−→F3.V+ same
F5: U+→F1.V− flip | U−→F4.U+ same | V+→F0.V− same | V−→F3.U+ flip
```

`schema/face_adjacency.toml` holds it. A script derives it independently from the face formulas; a core test asserts both agree and that all mappings are involutions.
**Corner rule:** a stencil cell out of range on both axes takes the mean of the two edge-adjacent stencil cells that exist, in the field's working arithmetic.

**Numerics.** Identity, pyramids and refinement use integers. Continuous queries use f64 with `libm`. Canonical integer position `Pos30 = (face, i30: u32, j30: u32)`. `LocalPos { cell, du: u32, dv: u32 (Q0.32), radial_m: f64 }`. **Cell measure** is solid angle (two spherical triangles, spherical excess). Statistics are **solid-angle-weighted**; surface-area weighting (needs the figure) is reserved.

### 10.3 V1 topology `veyra.topo.radial_1d/1` (FROZEN)

- Domain extent `R_ext` (metres, from the domain's `vertical: {kind: radius, extent_m}`).
- Level `ℓ ∈ [0,30]` has `2^ℓ` shells equally spaced in `r/R_ext`.
- `CellKey = (1<<ℓ) | i`, `i∈[0,2^ℓ)` (heap indexing). Parent = `key>>1`, children = `key<<1 | {0,1}`. Level = `63 − leading_zeros`.
- Centre `r=(i+0.5)/2^ℓ·R_ext`. Measure = shell volume `4π/3·(r_{i+1}³−r_i³)`.
- Tile: `2^min(T,ℓ)` consecutive shells; tile key `(ℓ, i>>T)`. Halo: 1 shell, with the ends clamped (zero-gradient).
- Interpolation: linear between shell centres; nearest at the ends.
- Pyramid: the same downsample operators as dir_cube, applied over child pairs.

### 10.4 How irregular and volumetric bodies fit later

- **Irregular star-convex bodies** (V1 fixture): `figure.kind = star_convex_radial` with a stored radius field on `dir_cube`. The body is an S² direction chart plus a radius per direction. **This only represents shapes where every ray from the centre crosses the surface once.** Concave bodies (contact binaries, overhangs) need `veyra.topo.mesh/1` and are explicitly reserved.
- **Volumetric** (stars, giants, comae, subsurface): `radial_1d` covers spherically symmetric profiles. Layered and octree topologies add vertical or full 3D structure. Fields keep the same descriptors, tile blobs, index entries (the key width is declared per index) and sampler API.
- **Atmospheres above surfaces:** `dir_cube_layered` with `vertical: height_above(surface_ref)`. Adding it changes no existing field.

### 10.5 V1 implements vs reserves

| Component | V1 | Reserved |
|---|---|---|
| Topologies | dir_cube, radial_1d | layered, octree, mesh, points |
| Figures | sphere, star_convex_radial, radial_profile_sphere | ellipsoid, mesh, implicit |
| Vertical kinds | none, radius | height_above, depth_below, pressure, optical_depth |
| Interpolation | nearest, bilinear/linear | higher order |

---

## 11. Reference frame, datum and surface model

There is no universal zero. Every height, depth and classification is defined **against a named reference declared by the body**.

**Frames** (V1):

| Frame | Definition |
|---|---|
| `universe_inertial` | Axes parallel to the universe lattice. Used as the base for rotation and dynamics. |
| `body_fixed` | Rotating; defined by `rotation` relative to a declared inertial frame (`uniform` in V1; reserved: precessing, tidally locked, tumbling, tabulated). Orientation is a unit quaternion at an epoch. |
| `primary_inertial(object)` | Axes parallel to `universe_inertial`, origin at the primary. Used for dynamics. |
| `region_origin(RegionKey)` | Origin at a region's centre, used for unbound objects. |

**Reference surfaces** are named entries in `body.json.reference_surfaces`:

| Kind | Definition | V1 |
|---|---|---|
| `sphere{radius_m}` | Constant radius. | yes |
| `offset_of{base, offset_m}` | A constant radial offset from another surface. | yes |
| `figure_surface` | The body's solid boundary, from the figure. | yes |
| `ellipsoid`, `equipotential{model}`, `isobar{pa}`, `isotherm{k}`, `optical_depth{tau, band}`, `radius_fraction` | physical definitions | reserved |

- A terrestrial body's "sea level" is `ocean.level`, an **ordinary named surface** declared via the `ocean` capability (`level_surface`). Heights in `topography` are measured from `topography.reference_surface`. Both are parameters, so a body can have no sea at all.
- A star declares `photosphere` as `sphere{radius_m}` with `semantic: "photosphere"` and a text `physical_definition` (e.g. `"Rosseland τ = 2/3"`). It is documentation only until `optical_depth` is implemented.
- "Land" is meaningful only if `ocean` is declared: `height_above(topography.reference_surface) > ocean.level offset`.
- "Hemisphere" is derived from the positive pole. "Season" language never appears in the core: climatology uses **orbit-phase slices** (§12.4).

---

## 12. Field, feature and capability registries

### 12.1 FieldDescriptor

```json
{
  "id": "0x01010001", "name": "topography.height_m", "capability": "veyra.cap.topography/1",
  "domain": "surface", "semantic": "scalar.height", "persistence": "invariant",
  "storage": {"dtype": "i16", "scale": "0.5", "offset": "0", "nodata": null},
  "unit": "m", "native_level": 9,
  "reference": {"surface": "datum.mean"},
  "temporal": {"kind": "static"},
  "sampling": {"interp": "bilinear", "below_native": "pyramid", "above_native": "refine"},
  "downsample": "mean",
  "refinement": {"algorithm": "veyra.refine.cdetail/1",
                 "params": {"roughness_field": "0x01010002", "f0_q16": 22938, "r_q16": 36045, "canonical_max_level": 24,
                            "guards": [{"kind": "threshold_partition", "threshold_surface": "ocean.level", "neighborhood": "3x3"}]}},
  "display": {"group": "Topography", "label": "Height", "colormap": "diverging_hypso", "range": [-8000, 8000], "legend": "continuous"},
  "provenance": {"stage": "topography"},
  "compat": "critical"
}
```

- `semantic` ∈ `scalar.*` (with unit) · `category` · `feature_ref` · `flags` · `vector.*` (reserved).
- `persistence` ∈ `invariant` · `periodic_mean` · `initial_state` · `dynamic`. **A baseline writer MUST reject `dynamic`.** Dynamic fields live only in simulation state.
- `dtype` ∈ `u8 i8 u16 i16 u32 i32 f32`. Decoded = `raw·scale + offset` in f64 (decimal-string `scale`/`offset`). `nodata` is a raw sentinel or null.
- `interp` ∈ `nearest | bilinear` (`linear` on 1D). `above_native` ∈ `refine | inherit | smooth_only | none`.
- `downsample` ∈ `mean | rms | min | max | sum | mode_lowest_tiebreak`, all in integer arithmetic (`mean` of 4 = `floor((a+b+c+d+2)/4)`).
- `temporal.kind` ∈ `static` · `periodic_slices{count, period_ref, origin_ref}` · `series` (reserved).
- `compat`: `critical` means a reader that cannot interpret `semantic`, `dtype` or `sampling` MUST refuse. `ancillary` means it MAY skip and a rewriter MUST preserve.
- Unknown JSON keys are preserved. Keys prefixed `x-` are never interpreted.
- The `display` block is optional. A field without it is inspectable but offers no default view.

### 12.2 Other field examples

```json
{"id":"0x01020001","name":"tectonics.plate","capability":"veyra.cap.tectonics/1","domain":"surface",
 "semantic":"feature_ref","persistence":"invariant","storage":{"dtype":"u16","nodata":0,"feature_table":"tectonics.plate"},
 "native_level":9,"temporal":{"kind":"static"},
 "sampling":{"interp":"nearest","below_native":"pyramid","above_native":"inherit"},"downsample":"mode_lowest_tiebreak",
 "display":{"group":"Tectonics","label":"Plates","colormap":"categorical_hash","legend":"feature_table"},"compat":"critical"}

{"id":"0x01040002","name":"climate.precipitation","capability":"veyra.cap.climate/1","domain":"surface",
 "semantic":"scalar.flux","persistence":"periodic_mean","storage":{"dtype":"u16","scale":"0.1","offset":"0","nodata":65535},
 "unit":"mm/month","native_level":7,
 "temporal":{"kind":"periodic_slices","count":12,"period_ref":"dynamics.orbital_period","origin_ref":"dynamics.periapsis","reduce_default":"mean"},
 "sampling":{"interp":"bilinear","below_native":"pyramid","above_native":"smooth_only"},"downsample":"mean",
 "display":{"group":"Climate","label":"Precipitation","colormap":"blues","range":[0,600],"legend":"continuous"},"compat":"critical"}

{"id":"0x01300001","name":"stellar.density","capability":"veyra.cap.stellar_structure/1","domain":"interior",
 "semantic":"scalar.density","persistence":"initial_state","storage":{"dtype":"u32","scale":"0.01","offset":"0","nodata":null},
 "unit":"kg/m3","native_level":10,"temporal":{"kind":"static"},
 "sampling":{"interp":"bilinear","below_native":"pyramid","above_native":"smooth_only"},"downsample":"mean",
 "display":{"group":"Stellar structure","label":"Density","colormap":"viridis","range":[0,160000],"legend":"profile_log"},"compat":"critical"}

{"id":"0x01100001","name":"ecology.state","capability":"veyra.cap.ecology/1 (EXAMPLE, reserved)","domain":"surface",
 "semantic":"category","persistence":"initial_state","storage":{"dtype":"u8","nodata":0,"vocab":"ecology.state/1"},
 "native_level":11,"sampling":{"interp":"nearest","below_native":"pyramid","above_native":"inherit"},
 "downsample":"mode_lowest_tiebreak","display":{"group":"Ecology","label":"Ecological state","colormap":"vocab","legend":"vocab_counts"},
 "provenance":{"limiting_factor_field":"0x01100010"},"compat":"ancillary"}
```

(Capability numeric IDs shown are illustrative of the allocation file's output; `schema/capability_ids.toml` is the single source.)

### 12.3 Feature model

A feature table is a schema'd table, not a raster.

- Columns are typed (`u32`, `u64hex`, `vocab`, `ref(table)`, `dec`, `geometry`).
- **Two IDs per row.** `stable_id` (u64) is derived from `ObjectId`, table and a generation-order-independent structural key (e.g. a plate's seed cell key). `ordinal` is a dense 1-based number assigned by sorting `stable_id` ascending (0 means none). Rasters store ordinals.
- **Geometry kinds:** `polyline_udeg` (V1; `[lat_udeg, lon_udeg]` i32 in the body-fixed display chart), `point`, `polygon_cells` (sorted CellKey list at a stated level), `graph` (edge-list columns), `volume_cells` (reserved).
- **Cell↔feature link:** forward by `feature_ref` rasters; reverse by scanning at a coarse level on demand; a `cover` column is reserved.
- **Storage:** `jcs-json` up to 10⁴ rows; a `columnar-v1` blob kind is reserved for rivers, caves, catchments, jets and vents.
- V1 tables: `tectonics.plate`, `tectonics.boundary`.

### 12.4 Time and periodic slices

- `UTime` is an `i128` count of nanoseconds, stored as a decimal string.
- `periodic_slices` slice `k` covers fraction `[k/n, (k+1)/n)` of the period named by `period_ref`, starting from the phase named by `origin_ref` (`dynamics.periapsis`, `rotation.epoch`, …). Slice labels are neutral (`S0…S11` or phase percentages). Calendar names never appear in the core. A body without an orbit cannot declare orbit-based slices. The validator checks that the references resolve.

---

## 13. Sub-resolution authority

### 13.1 Principle

> **A consumer engine MUST NEVER invent canonical physical state.**
> Whatever is defined below the finest stored level is defined by a versioned, normative function in the core. Consumers call it. They do not reimplement it.

| Class | Defined by | Bit-compared | Users |
|---|---|---|---|
| **C0 canonical** | Stored data plus a registered refinement algorithm (pure, integer, versioned). | **Yes, must match exactly.** | Collision, physics, gameplay, simulation, navigation, edits |
| **C1 derived relief** | Pure functions of C0 (normals, slope, curvature) and optional micro-relief with an amplitude cap `visual_relief_max` (default 0.10 m). | Formulas: yes. Micro-relief: no. | Rendering and shading only |
| **C2 optical** | Anything (albedo noise, detail normals, vegetation sprites, grain). | No | Presentation only |

**Law:** anything read by gameplay, collision, simulation or persistence MUST be C0. A consumer MAY add C1/C2 and MUST NOT add C0. Above `canonical_max_level`, or for a field with `above_native: smooth_only | none`, there is **no canonical detail** to invent.

### 13.2 Registration

Refinement algorithms are registered in the core by ID (`veyra.refine.<name>/<n>`). Each declares the topology and field semantics it supports. A field names one in its descriptor. Unknown algorithm means refuse (it is in `required_features`). **`/n` semantics are immutable forever.** Any change, even one rounding rule, ships as `/n+1`; the core may implement several versions at once.

### 13.3 V1 refinement set

| `above_native` | Algorithm | Applies to |
|---|---|---|
| `refine` | `veyra.refine.cdetail/1` | height-like scalar fields on `dir_cube` with a roughness companion (terrestrial height; radius-like fields later) |
| `inherit` | built-in `inherit` | categorical and `feature_ref` fields: a finer cell takes the value of its native ancestor, with no jitter |
| `smooth_only` | built-in | continuous fields: native-lattice interpolation, no invented detail |
| `none` | | radial_1d fields in V1 |

### 13.4 Arithmetic, inputs and seeds

All C0 refinement is integer arithmetic on i64 in **working units of 2⁻¹⁰ of the field's unit** (`scale·1024` MUST be an integer). Inputs are exactly: the stored native raw lattice and its neighbours via adjacency; the stored roughness lattice at the same native level (validator-enforced); the body seed; the field name; the params. Nothing else (camera, request order, cache, thread, host) may influence C0.

```
body_seed        = blake3.derive_key("veyra.seed.v1", object_id ‖ "body")[0..8]  (u64 LE)
subseed(purpose) = blake3.derive_key("veyra.seed.v1", body_seed_le8 ‖ utf8(purpose))[0..8]
purpose          = "refine/<field name>"
mix64(x)         = SplitMix64 finaliser (wrapping u64): z=(x^(x>>30))*0xBF58476D1CE4E5B9; z=(z^(z>>27))*0x94D049BB133111EB; z^(z>>31)
detail_hash(parent_key, q) = mix64( mix64(subseed ^ parent_key) + q*0x9E3779B97F4A7C15 )   (wrapping)
```

Randomness is a pure function of (body seed, field, parent CellKey, child index). It never depends on traversal order, so inserting a stage, field or level reshuffles nothing.

### 13.5 `veyra.refine.cdetail/1`

For parent cell P at level `l ≥ N` (native level), value `p`, 3×3 neighbourhood `n[dx][dy]` (stored for `l=N`, refined above), and child `q=(a<<1)|b` with `sa = a?+1:−1`, `sb = b?+1:−1`:

1. **Interpolate (scale 16):** `I_q = 9p + 3·n[sa][0] + 3·n[0][sb] + n[sa][sb]`.
2. **Conserve:** `C = 64p − ΣI_q`; `J_q = I_q + floor_div(C,4) + (q < C mod 4 ? 1 : 0)`, so `ΣJ_q = 64p` exactly. `base_q = floor(J_q/16)`, `rem_q = J_q − 16·base_q`, `R = 4p − Σbase_q ∈ {0..3}`; add `+1` to the R children with the largest `rem_q` (ties → lowest q).
3. **Detail:** `h_q = ((detail_hash(P,q) >> 40) as i64) − 2²³`; `d = ZeroSum4(h)`; `e_q = (d_q·a_l) >> 23` (arithmetic shift); `detail = ZeroSum4(e)`.
   `ZeroSum4(v)`: `S=Σv; m=floor_div(S,4); w_q=v_q−m; r=S−4m; for q<r: w_q −= 1` → `Σw = 0` exactly.
4. **Child:** `c_q = base_q + detail_q`. **`Σc_q = 4p` exactly**, so every stored cell is the exact mean of its descendants at all depths.

**Amplitude.** `σ` = dyadic-bilinear interpolation of stored roughness over native cells: with `m=2^(l−N)`, parent's local index `(ix,iy)`, `tx = 2ix+1−m`, `ty = 2iy+1−m`; weights along x: self `2m−|tx|`, neighbour (direction `sign(tx)`, `+` if 0) `|tx|`; same along y; `σ = Σ wx·wy·r / (2m)²` with floor division; neighbours via adjacency. `a_l = (σ·decay[k]) >> 16` with `k=l−N+1`, `decay[1]=f0_q16`, `decay[k]=(decay[k−1]·r_q16)>>16`. Roughness is defined as the standard deviation of sub-cell height at native scale. `f0` and `r` are generator-tuned data, part of identity.

### 13.6 Guards (generalized; no "sea")

A guard is an **invariant-set constraint** declared in field params. V1 kind: `threshold_partition{threshold_surface | threshold_value}`. A native cell is *boundary* iff its 3×3 native neighbourhood contains values on both sides of the threshold. For descendants of a **non-boundary** cell, the **output** value (not the recursion value) is clamped to the centre's side of the threshold (one working unit past it). The classification of coarse cells therefore cannot change below the stored level. Coastlines, shorelines or any other partition boundaries cannot appear or drift except inside boundary cells.

- A body with no `ocean` capability simply declares no such guard.
- `guard_rate` MUST be 0 in conformance bodies and `< 1e-4` in generated bodies. The validator reports it.
- Generator invariant **I-R1:** for non-boundary native cells, `σ ≤ (distance_to_threshold − margin)/K` (K from the spec table in `docs/spec/refinement.md`).
- **Pinned features** (reserved): `refinement.post_steps` (empty in V1) will apply deterministic carves (rivers, faults, cave entrances) after step 4, reading only stored feature geometry. A refinement function never moves a feature.
- Overflow of the stored dtype raises `E_REFINE_RANGE`. It never wraps.

### 13.7 Evaluation, determinism, caching

- `plan()` for a refined tile returns the native tiles and halo ring required. Evaluation is top-down per level; results are cached in RAM keyed by `(baseline_id, extension set hash, field, tile key)`. Caches are never authoritative.
- **MUST be bit-identical on every target:** CellKey encoding, adjacency, hashing, blob encoding, pyramids, all C0 refinement integers, `inherit`, feature resolution, seed derivation. They are integer-only or IEEE-exact (`+ − × ÷ sqrt`).
- **Specified f64, bit-identical because one source compiles to every target (no FMA, no fast-math, `libm` transcendentals):** continuous `sample()`, cell centres, derived slope, Kepler propagation.
- **Free to differ (presentation only):** GPU filtering, f32 rendering, colour mapping, C1 micro-relief, all C2.
- **Pinning:** `conformance/refine_vectors/` holds the BLAKE3 of canonical refined tiles at fixed keys. A change fails CI.

---

## 14. Orbital and interstellar state

### 14.1 What each side owns

| Item | Owner | Where stored | Notes |
|---|---|---|---|
| Intrinsic dynamical parameters: GM, non-gravitational parameters, rotation model, propagator class | **Body baseline** | `dynamics/descriptor.json` | Immutable. |
| Initial dynamical state at materialization time `T_m` | **Body baseline** | `dynamics/origin.json` (origin keyframe, derived by evaluating the recipe at `T_m`) | Immutable; continuity with the procedural trajectory. |
| **Current dynamical state** (keyframes and events after `T_m`) | **Body dynamics ledger** | `state/dynamics/ledger.jsonl` | **Sole writer: the universe dynamics service** through `commit_keyframe`. Body-local simulation MUST NOT write it. It submits requests (outgassing impulse, maneuver, impact) that the service validates and commits. |
| Pre-materialization trajectory | **Universe recipe** | analytic function | Procedural truth until M4. |
| Global time | **Universe** | `state/time.json` | |
| System membership and relations | **Universe** | `registry/relations.ledger.jsonl` | A relation, never a body property. |
| Spatial/system index | **Universe, DERIVED** | `registry/index/`, `state/dynamics_index/` | Rebuildable from ledgers and recipe; the validator recomputes and compares. |

**Resolution of "who is current truth":** the dynamical truth of a materialized body is *its own ledger head*. The universe holds only (a) the ordering and orchestration authority (the single writer and the time), and (b) a **derived index**. The universe never stores a second position or velocity for a body. Between ledger commits, live n-body state exists only in the running dynamics service. It is simulation runtime, not persistence. A checkpoint forces commits (§15).

A body opened **standalone** (Inspector, export) is complete: baseline descriptor + origin + ledger = full dynamical meaning, with no universe required.

### 14.2 State representation

```
DynState {
  epoch: UTime,
  reference: primary(ObjectId) | region(RegionKey)  ,   // frame origin: a primary body, or a region centre for unbound objects
  r_um:  [i128; 3],     // position in micrometres relative to the reference, axes parallel to universe_inertial
  v_um_s:[i64; 3],      // velocity in µm/s
  spin:  optional orientation + angular velocity
}
Keyframe { seq, kind: cartesian_state|keplerian_elements, state, propagator, cause, classification }
```

- **Fixed-point integers** make persisted states exact and platform-independent (an i128 of micrometres spans ≫ 1000 light-years).
- A **reference change** (captured by a star, ejected, leaves a region) is an **exact integer re-expression** (`r' = r + (offset of reference origins)`), recorded as an event keyframe `{cause: "reference_change"}`. Nothing about the body's identity changes.
- `classification` (`bound{primary}`, `unbound`, `escaping`) is **derived** from specific orbital energy relative to the reference and is recorded at the keyframe as a labelled convenience. The validator recomputes it.
- **Propagation:** `state(t) = propagate(latest keyframe ≤ t, propagator, t)`. V1 implements `veyra.prop.kepler2/1` (elliptic two-body, universal-variable, f64 + `libm`). Hyperbolic, n-body ephemeris segments and non-gravitational forces are reserved. For `t` earlier than the first ledger entry the answer is flagged `source: recipe_backcast`.
- **Writes:** `commit_keyframe(object_id, keyframe)` is the only function that appends to a dynamics ledger. It checks the continuity of the previous head and the energy/classification.

### 14.3 Moving between systems

A comet is a body with an `ObjectId` whose reference may be `primary(star A)`, then `region(R)` after ejection, then `primary(star B)`. Each change is a ledger event. Its artifact does not move. `relations.ledger` records `left(A)`, `joined(B)` so the universe can answer membership queries, while the body remains the same object at the same path. A body artifact is an object, not a file belonging to a system folder.

---

## 15. Persistence

| Layer | Mutability | Storage | Identity | Writer |
|---|---|---|---|---|
| **Baseline** | immutable | `body.json` + indexes + blobs | `BaselineId` | materializer, once |
| **Sealed extensions** | append-only, immutable entries | `extensions/ledger.jsonl` + blobs | `ExtId`s, ledger head | materializer (M5), rare |
| **Edit layer** | append-only | `state/edits/` | `EditHead` | edit service |
| **Simulation state** | mutable via snapshot + journal | `state/sim/` | `SimId` | body-local simulation |
| **Dynamics ledger** | append-only | `state/dynamics/` | `DynHead` | dynamics service only |
| **Universe state** | mutable ledgers | `registry/`, `state/` | ledger heads | universe |
| **Player/session** | mutable | `state/players`, `sessions` | | game |
| **Caches** | disposable | `cache/` | none | anyone |
| **Save/Checkpoint** | immutable manifest | `state/saves/` | `SaveId` | save service |

- **Edits are semantic** (`excavate(region, shape)`, `place_entity`, …) with a raster-overlay fallback, using the same blob/index scheme. Edits reference `BaselineId` and never alter it.
- **Dynamic fields** live only in sim state.
- **A save is a binding of heads**, not a copy: universe ledger heads + per body `{object_id, baseline_id, ext head, edit head, sim snapshot, dyn head}` + time + player state. Snapshots (non-append-only state) are blobs. `SaveId = b3(JCS(manifest))`. `BodyStateId` is the same construction per body.
- **Atomicity:** ledgers are appended with fsync then the head is the last complete line. Replaced files (`time.json`) are written to a temp name and renamed. Tools verify chains on open.
- **Frozen-baseline rule:** a save is bound to its baselines forever. Generator improvements create new bodies. Rebasing edits onto regenerated terrain is not supported.
- **Authored overrides** (artistic corrections) are a labelled edit class with provenance `authored`. Nothing is fixed silently in a consumer.

---

## 16. Provenance and diagnostics

### 16.1 Provenance (four tiers, generalized)

| Tier | Content | V1 |
|---|---|---|
| 1. Static dependency DAG | `provenance/dag.json`: nodes `{id, kind: field|stage|table|capability, algorithm, params_hash}`, edges `{from, to, role}`. Generator stages are honest names. | yes |
| 2. Components and dominance | Component fields with **exact identities** (`height = iso + tec + surf`); `dominant_process` category; limiting-factor IDs for ecology (reserved). | terrestrial: yes |
| 3. Feature causal links | Rasters linking cells to feature rows (`nearest_boundary`) plus typed edges between features (boundary→plates). | yes |
| 4. Explain recipes | `provenance/explain.json`, data-driven traversals contributed by **capabilities** (the stellar capability supplies its own, e.g. density ← composition ← mass). A re-evaluating **explain mode** is optional, dev-only and non-canonical (reserved). | recipes only |

`explain(pos, subject_field)` returns a structured chain of `{label, values, feature_expansions}` plus template text. The UI states honestly that it shows *what contributed and by how much*, not counterfactuals.

### 16.2 Diagnostics

`diagnostics/diag.json` (keyed to `baseline_id`, read only on match; no compatibility guarantee beyond `diag/1`):

```json
{"schema":"veyra.diag/1","baseline_id":"bas:b3:…",
 "stages":[{"id":"tectonics","algorithm":"…","duration_ms":812,"status":"ok","warnings":[],"metrics":{"plates":14}}],
 "residuals":[{"name":"height_component_sum","field":"0x01010001","value":"0","unit":"raw","threshold":"0"}],
 "solver":[{"stage":"isostasy","iterations":37,"converged":true,"final_residual":"1.2e-6"}],
 "snapshots":[{"stage":"crust","field":"0x01010001","index":"diagnostics/index/s_crust_height.idx"}],
 "internals":{"x-vgen":{}}}
```

- Snapshot tiles use the **same blob scheme**, so unchanged regions dedupe against the canonical blobs, and the `final` snapshot IS the canonical index.
- Diagnostic field IDs are in the `0xF000+` capability range and never appear in the baseline registry.
- The sampling path never reads diagnostics. `veyra body validate --write-diag` records its own measurements in `diagnostics/validation.json`.

---

## 17. Reference core

### 17.1 Language and toolchain

**Rust**, pinned to the exact stable release current at Stage 0 (`rust-toolchain.toml`). Targets: `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `wasm32-unknown-unknown`. Why Rust over C++: one workspace, no build system per platform, memory safety in a parser that ingests files from disk and network, first-class WASM. Unreal needs only a C ABI static library.

Determinism rules (enforced by lint and CI):

- `#![forbid(unsafe_code)]` in `veyra-core`. Only `veyra-capi` may use `unsafe`.
- `clippy.toml` `disallowed-methods`: `sin cos tan asin acos atan atan2 exp ln log pow powf powi mul_add` on floats, and `HashMap`/`HashSet` iteration in any hashed path (use `BTreeMap`).
- Transcendentals only from `libm`. Canonical float ops are limited to `+ − × ÷ sqrt`. No fast-math. No FMA contraction.
- Reader-path dependencies are pure Rust: `blake3`, `ruzstd` (decode), `serde`/`serde_json` + own JCS writer, `libm`. The native-only writer may use `zstd`.

### 17.2 Crates and dependency direction

```
veyra-core            all semantics; sans-IO; native + wasm32; no fs, no network (fs helpers behind feature "std-io")
veyra-writer          native; builds body artifacts; Rust API + C ABI (veyra_writer.h)
veyra-universe        native + wasm32-capable: recipe contracts, ObjectId, ledgers, registry, dynamics service, materialization orchestration
veyra-bodygen         native; body generators (terrestrial_test, star_lite) implemented over veyra-writer
veyra-cli             `veyra` binary
veyra-wasm            wasm-bindgen bindings over core; no logic of its own
veyra-capi            cbindgen C ABI over core (shell in V1)
veyra-conformance     corpus builder and runner

veyra-bodygen ──► veyra-writer ──► veyra-core
veyra-universe ──► veyra-core, veyra-writer   (calls bodygen via a registry trait; bodygen depends on writer, not on universe)
veyra-cli ──► all native crates
veyra-wasm, veyra-capi ──► veyra-core
inspector (TS) ──► veyra-wasm
unreal adapter (later) ──► veyra-capi
```

`veyra-core` MUST NOT depend on a generator, an engine, the filesystem (outside `std-io`), or the network.

### 17.3 Core module map

```
ids, time, canon/{jcs,hash,blob,index,ledger}, topology/{mod,dir_cube,radial_1d,adjacency},
frames, model/{body,capability,domain,registry,vocab,figure,surfaces},
io/{need,loader,store}, sample/{lattice,interp,time,sampler,tile,stats}, views/{catalog,derived},
refine/{fixed,seed,cdetail,guards,inherit}, features, provenance/{dag,explain},
dynamics/{state,keyframe,kepler2,ledger}, validate/{rules,report}
```

**Contract immutability.** Any versioned identifier (`veyra.topo.*`, `veyra.refine.*`, `veyra.prop.*`, `veyra.ugen.*`, `veyra.gen.*`, `veyra.cap.*`) has immutable semantics once released. Behaviour changes ship as a new version number. The core may carry several versions concurrently so old data keeps opening.

### 17.4 Sans-IO API (sketch)

```rust
// loading
Need { Section{path,hash}, Index{field,hash}, Blob{hash}, Ledger{name} }
BodyLoader::begin(body_json:&[u8]) -> Result<(BodyLoader, Vec<Need>)>
BodyLoader::provide(&mut self, need:&Need, bytes:Vec<u8>) -> Result<Vec<Need>>   // verifies hashes
BodyLoader::finish(self) -> Result<Body>

// metadata / discovery
Body::object_id() / baseline_id() / classification() / physical() / figure() / frames() / reference_surfaces()
Body::capabilities() / domains() / fields() / field(id|name) / vocab(name) / feature_tables() / views() -> Vec<ViewDescriptor>
Body::dynamics() -> DynamicsView         // descriptor, origin, ledger head, state(t)

// sampling
SampleQuery { field, pos: Pos, level: LevelSel, time: TimeSel }
Pos { Dir{x,y,z}, AxialLatLon{lat,lon}, Cell(domain,key), Local(LocalPos), Radial{r_m}, Pos30(..) }
LevelSel { Native, Exact(u8), Canonical }     TimeSel { Static, Slice(u8), Mean, Phase(f64) }
Body::plan(&SampleQuery|&TileRequest) -> Vec<Need>        // pure
Body::provide_blob(hash, bytes) -> Result<()>
Body::sample(&SampleQuery) -> Result<Sample, Missing(Vec<Need>)>
  Sample { value, level_used, source: Stored|Pyramid|Refined|Inherited|Const|Nodata, cell, raw, category, feature }
Body::tile(&TileRequest{field,key,time,halo,view}) -> Result<TileData,Missing>   // view: Raw|FeatureAttr|Derived|TimeReduce
Body::domain_geometry(domain, key, grid_n, relief_scale) -> Result<Geometry,Missing>   // positions from figure + relief; the viewer never computes shape
Body::histogram/stats(view, level, time)       // weighted by the domain measure
Body::inspect(pos, opts) -> PointReport        // every field in the domain at the point
Body::explain(pos, subject) -> Explanation     Body::provenance_dag()
Body::feature(table, ordinal|stable_id) / feature_geometry(...)

// universe-facing (veyra-universe)
Universe::open / id / status(object) / resolve(object, level) / materialize(object) / body(object) / dynamics().commit_keyframe(..)
validate::run(&dyn BlobSource, ValidateLevel) -> Report
```

C ABI mirrors this with opaque handles and stable numeric error codes plus a `missing` out-list. WASM exposes the same calls with typed-array transfer.

---

## 18. Validator and conformance

### 18.1 Conformance corpus

All corpus bodies use `tile_log2=3`, native levels ≤ 5, and are generated by `veyra conformance gen` (which is also a test). Total < 3 MB. All are **standalone fixtures**.

| Fixture | Content | Proves |
|---|---|---|
| `cb0-addressing` | dir_cube field storing each cell's own `(face,i,j)`; pyramid; radial_1d key checks | CellKey encode/decode, face selection, diagonal ties, tile keys, index order |
| `cb1-seams` | Smooth analytic fields + checker-per-face | 24 edges + 8 corners, halos, bilinear continuity, corner rule |
| `cb2-categories` | category + nodata + ties | categorical sampling, vocab, pyramid ties |
| `cb3-features` | 3 plates, 4 boundaries, nearest-boundary links | ordinals/stable IDs, feature_ref, geometry, explain chain |
| `cb4-refine` | native level-3 height + roughness with a partition boundary and a land interior | exact mean conservation 8 levels deep, ZeroSum4, guards, pinned tile hashes, order independence |
| `cb5-hashing` | const tile, duplicate tiles, pretty-printed JSON | JCS, blob hash, const entries, dedup, literal BaselineId |
| `cb6-compat` | unknown ancillary field (preserved), unknown critical field/capability (refuse), minor+1 (open), major+1 (refuse), tampered blob | evolution and tamper rules |
| **`cb7-star1d`** | **No solid surface, no datum, no figure_surface.** `radial_profile_sphere`, domain `interior` on radial_1d, `stellar_structure` fields (density, temperature, pressure, hydrogen fraction), `photosphere` reference surface, explain recipe | A body with no terrain, no sea, no seasons, no dir_cube; volumetric profile sampling; capability-driven views |
| **`cb8-rock`** | `star_convex_radial` figure from a stored radius field (non-spherical), `solid_surface` + `surface_material` + `thermal_state`, **no height datum, no ocean, no climate**, no orbit | Irregular shape; terrestrial concepts absent; derived geometry from the figure |
| **`cb9-minimal-void`** | Valid body with **zero capabilities and zero fields** | Nothing is mandatory except identity, physical, figure, frames, dynamics |
| `cu0-universe` | ObjectId vectors, address codec, ledger replay, M0–M3 sketches, commit crash tests, dynamics keyframes, relation events | Identity, lifecycle, atomicity, authority |

`conformance/queries/*.jsonl` holds queries; `expected/*.jsonl` holds results (f64 as hex bits). **Native and WASM agree** if the same query file produces byte-identical output through `veyra conformance run` and a Node harness over `veyra-wasm`. Both are compared against `expected/`. Windows, Linux and macOS run natively in CI.

### 18.2 CLI (`veyra`)

```
veyra body validate <body> [--level fast|standard|full] [--json] [--strict] [--write-diag]
veyra body verify   <body>                  # hashes only
veyra body info|fields|views <body>
veyra body sample   <body> --field F (--dir x,y,z | --axial-latlon LAT,LON | --cell DOMAIN:KEY | --radius M) [--level N|native|canonical] [--slice K|mean]
veyra body tile     <body> --field F --key L:KEY [--halo H] --out file.bin
veyra body inspect|explain <body> <position args>
veyra body dynamics <body> [--t UTIME]
veyra body diff <A> <B>                     # manifest-level
veyra body export   <body> --out DIR        # self-contained closure
veyra universe init|info|verify|repair <universe>
veyra universe resolve <universe> --object obj:… --to catalogued|system|orbital
veyra universe status|list <universe> [--status ...]
veyra universe materialize <universe> --object obj:…
veyra universe commit-keyframe ...          # dev tool through the dynamics service
veyra conformance gen|run|diff
veyra fmt <path> --pretty|--canonical
```

Exit codes: 0 ok · 1 warnings (2 with `--strict`) · 2 errors · 3 unreadable.

### 18.3 Validator levels

| Level | Checks |
|---|---|
| fast | JSON Schema validity; no JSON floats in hashed files; section and index hashes; `required_features` known; capability IDs resolve and `requires` satisfied; field IDs unique and match capability allocation; vocab and table references; domain/topology/frame/surface references resolve; reference surface graph acyclic |
| standard | + every blob exists, decompresses, hash matches, header matches registry; index sorted and unique; tile coverage complete at each declared level; const legality; dtype range; nodata policy; ordinals exist; declared component identities exact; roughness/height native level equal; `persistence ≠ dynamic`; class profile satisfied if declared; **no field semantic contradicts declared capabilities**; dynamics descriptor and origin consistent (classification recomputed); `origin.sketch_hash` matches when from a universe; ledgers chain-valid |
| full | + pyramid recomputation (exact), **seam statistics** vs interior, refinement self-check (mean conservation, guard rate, I-R1), DAG acyclic and complete, explain recipes resolve, determinism check (rebuild sampled tiles, compare hashes), derived-index agreement (universe), baseline-only vs state-layer separation |

---

## 19. Inspector

### 19.1 Product behaviour

Simple browser page: near full-viewport 3D view; top toolbar with a **cube icon + Debug** dropdown; compact legend/info card at top-right; orbit, zoom, click to inspect. Switching a view recolours the same canonical data. The legend shows mode name, description, colour coding, category types with **measure-weighted counts**, and statistics. No design system.

### 19.2 Body-aware architecture

- The Inspector opens **one body artifact** by URL (`?body=…/obj_….veyra/`). Universe browsing is not V1.
- Everything semantic comes from the core through the worker: `body_summary()` (class, capabilities, domains, figure, frames, dynamics), `views()` (a **ViewDescriptor** list built by the core from field descriptors and capability-declared derived views), `tile(view, key, halo)`, `domain_geometry`, `stats(view)`, `inspect(pos)`, `explain(pos, subject)`, `features(...)`, `diagnostics()`.
- **Two view kinds in V1**, selected by the **domain topology** reported by the core, not by body class:
  - `surface_mesh` (dir_cube domains): a globe mesh from `domain_geometry` (shape from the figure, relief from the declared height-like field), coloured by a tile texture + LUT, with a quadtree LOD.
  - `radial_profile` (radial_1d domains): a 2D radial cutaway disc coloured by shell value, plus a line profile chart in the info card.
- **ViewDescriptor** = `(domain, field | derived id, operator, time selection, label, group, description, legend kind, colormap, range)`. The dropdown groups by capability `display.group` and lists only what the body has. No capability means no group.
- Colour: the worker returns the view as an R32F tile (halo 1). The fragment shader maps via a 256×1 LUT built from descriptor metadata (continuous or vocabulary colours; categorical uses NEAREST + `texelFetch`). Changing view swaps texture and LUT only. Shading normals come from the core's C1 derived layer.
- Picking: raycast the displayed mesh (fallback: the declared reference surface or the figure bounds), send a direction or radius to the worker. `inspect()` reads canonical data, never pixels.
- Info card: view info + legend + stats, then a collapsible **Selected point** block (all fields at that position with source: stored / pyramid / refined / inherited and level used), the dynamics summary (class, reference, `state(t)` at current universe time if available), and the **explain** chain from capability recipes.
- Overlays: feature geometries from `feature_geometry`, coloured by vocab, toggled in the toolbar if any feature table with geometry exists (terrestrial: plate boundaries).
- Diagnostics: if `diagnostics/diag.json` matches the baseline ID, a Diagnostics group and a **Stage** selector appear for fields with snapshots.
- Failure: hash mismatch, unknown critical feature, unsupported version → a visible error card with the validator error code. No silent fallback.

### 19.3 Implementation

Vite + TypeScript + three.js (WebGL2), no UI framework, one dedicated Worker hosting `veyra-wasm` for **all** body queries; typed arrays are transferred, not copied. Loading is sans-IO: the worker `fetch`es `body.json`, calls `begin`, fetches each needed section/index/blob over plain static HTTP (6 in parallel), `provide`s, and retries. Content-addressed URLs are cache-forever. No range requests and no server code in V1. Decoded-tile LRU budget about 256 MB. WebGPU, compare mode, mobile controls and offline pack loading are non-goals for V1.

**Rule:** the Inspector may do display math only (LUTs, camera, texture upload). It MUST NOT compute cell keys, projections, interpolation, derived layers, refinement, feature resolution, propagation or class logic.

### 19.4 V1 debug views (generated)

| Group (from capability) | Views | Source |
|---|---|---|
| Spatial (any dir_cube domain) | Cube faces · Tile level · Axial latitude bands (15°) | topology-derived |
| Topography | Height · Roughness · Slope · Isostatic · Tectonic · Surface-process · Dominant process | fields + `derived.slope` |
| Ocean | Land vs ocean (requires `ocean` and `topography`) | capability-declared derived |
| Tectonics | Plates · Boundary distance · Boundary class · Crust type · Crust age | fields + feature attr |
| Climate | Temperature · Precipitation (slice: Mean, S0…S11) | fields |
| Stellar structure | Density · Temperature · Pressure · Hydrogen fraction (radial view) | fields |
| Solid surface / Surface material / Thermal state | Radius (figure) · Surface material · Surface temperature | fields |
| Diagnostics (if present) | Height by stage | diag |

A **new stored field** appears the moment its descriptor has a `display` block. Hundreds of future fields cost one descriptor and one vocabulary each. New *view kinds* (vector flow, volume slice, mesh) are the only Inspector work.

---

## 20. V1 implementation scope (ruthless)

**Built in V1**

| Area | V1 |
|---|---|
| Format | Body artifact directory form; universe directory form; JCS + BLAKE3; blobs; indexes; ledgers |
| Core | ids, time, canon, topologies (dir_cube, radial_1d), model, sans-IO loader, sampler, views, refinement `cdetail/1` + inherit + smooth_only, features, provenance tiers 1–4 (recipes), dynamics types + kepler2 (elliptic) + ledger, validator |
| Capabilities | solid_surface, topography, tectonics, ocean, climate, surface_material, thermal_state, stellar_structure |
| Universe | `ugen.minimal/1`, lattice index, ObjectId, M0–M4 working, M5 only as pure refinement, M6 ledgers present but edits/sim empty, relations ledger, registry ledger |
| Generators | `terrestrial_test/1` (plates, uplift, simple climate; honest stage names, placeholders labelled), `star_lite/1` (analytic profile). Rock fixture is conformance-only |
| Consumers | CLI; WASM/Node; browser Inspector |
| Parity | native vs WASM bit-identical on the corpus and materialized bodies |

**Test profile (a parameter set, not the format):** `terrestrial_test`: radius 6.0×10⁶ m, GM about 4×10¹⁴ m³/s², rotation period 86 400 s, native levels height L9 (cell ≈ 20 km), climate L7, `tile_log2=7`, high profile L10.

**NOT built in V1:** edit service, sim state, packed `.veyra` / `.veyra-universe`, Unreal adapter, `layered`/`octree`/`mesh`/`points` topologies, any capability beyond the eight listed, hyperbolic or n-body propagation, comet/giant/dead-moon generators, galaxy structure, universe browsing in the Inspector, compare mode, WebGPU, multiplayer authority, migration tooling, sealed extension writers.

---

## 21. First end-to-end acceptance

Deterministic and observable. A CI script (`scripts/acceptance.ps1` + `.sh`) runs it.

1. `veyra universe init --seed 0x… ` creates the universe. `universe.id` equals `b3(JCS(universe.json))`. Running it twice gives the same `UniverseId`.
2. `veyra universe resolve … --to orbital` on a seeded region yields ≥ 1 system. Terrestrial candidate **T** and star **A** (Auren) are `orbital_resolved`, with **stable ObjectIds identical across two runs and two OSes**, and **no `.veyra` directory exists** for them. The universe directory contains no body storage for the other unresolved objects.
3. `veyra universe materialize --object T` commits atomically. The registry ledger has the `materialized` record with the `baseline_id`; `materialized/obj_<T>.veyra/` exists; `veyra body validate --level full` exits 0 with `guard_rate < 1e-4`, exact component identities, seams within threshold, `sketch_hash` matching. Materializing A produces a valid stellar body.
4. **Reopen without regeneration:** run `VEYRA_GEN_PERTURB=1 veyra universe materialize --object T` (a test switch that makes the generator output differ). The command refuses ("already materialized") and `body.id` is unchanged. `veyra universe open` returns the same `BaselineId`. A temporary build of `veyra-bodygen` with the generator removed still opens the body.
5. A static HTTP server serves the body directory. The Inspector opens **T**: a globe with a legend whose stats are measure-weighted; the Debug dropdown lists exactly the groups of T's declared capabilities. Opening **A** shows the radial profile view and **no** Topography, Tectonics, Ocean or Climate groups. Opening `cb9-minimal-void` shows only the Spatial/none state.
6. A Playwright run selects every dropdown entry for T, A and `cb8-rock`, asserting a non-blank render, no console errors, and legend category counts equal to `veyra body` histograms.
7. Plate boundary overlay segment count equals the polyline vertex count in `tectonics.boundary`.
8. Zooming to native+4 shows refined C0 terrain, continuous across tile and cube-face edges (no crack > 1 px at the seam test cameras).
9. Click the highest peak: `inspect` returns stored/refined values matching `veyra body sample`; `explain` returns a chain with the dominant process, tectonic component, nearest **convergent** boundary, both plates, distance. A click in a basin returns a different chain.
10. **Native vs WASM:** ≥ 200 fixed points over six faces and seams, including `Canonical` refined heights and `state(t)` for both bodies, are **bit-identical** (f64 as hex bits) between `veyra body sample` and the browser worker output.
11. `veyra body export T --out X` then opening X with the universe and store directories hidden works and validates. The exported `baseline_id` equals the original.
12. Delete all `cache/`. Reloading changes nothing. Corrupting one blob byte makes validator and Inspector fail with the same error code and blob hash.
13. Stage select (crust → tectonics → final) changes the height globe. `final` matches canonical height byte-for-byte.
14. `veyra universe verify` recomputes the derived indexes and finds no difference. A manually injected second "position" in the index is detected.
15. The non-terrestrial fixtures `cb7-star1d`, `cb8-rock`, `cb9-minimal-void` pass validation `full`, native/WASM parity and Inspector smoke tests with **zero terrestrial concepts** in their registries (checked by a test scanning for forbidden core names: `elevation`, `sea`, `ocean` etc. appear only inside capability schemas).

---

## 22. Scale path

- **Richer terrestrial bodies:** add capability schemas (geology, hydrology, atmosphere, soils, ecology, caves…), vocabularies, feature tables (rivers as columnar graphs, catchments as `polygon_cells`), and explain recipes. Zero Inspector code. Native-level increases are pure scale-ups.
- **Stars:** add `radiative_zone`, `convective_zone`, `photosphere`, `corona`, `stellar_activity`, `stellar_wind`, `spectral_output` schemas. Extend with `octree_vol` for 3D activity. Same artifact.
- **Comets and irregular bodies:** `nucleus_shape` via star-convex now and `mesh` topology later; volatile, jet and coma fields; dynamics with non-gravitational parameters; hyperbolic propagation. The unbound reference form already exists.
- **Gas and ice giants:** `radial_1d` interior + `dir_cube_layered` or `octree_vol` atmosphere; `cloud_layers`, `circulation`, `chemistry`.
- **Irregular and volumetric spaces:** new topology implementations behind the same trait. No field, index or body-identity change.
- **Many systems:** the registry ledger is sharded by region; the derived index scales; unmaterialized space costs nothing.
- **Edits and simulation:** the layers exist as ledgers in V1; services and semantic edit operations follow. `dynamic` fields live in `state/sim`.
- **Packed `.veyra` / `.veyra-universe` / packed save:** the same logical closure in a range-readable archive (index + blobs + ledgers), with a `BlobSource` implementation. No hash changes.
- **Unreal:** `veyra-capi` plus materializers that cache by `(baseline_id, ext head, region, materializer version)`. Unreal proposes edits through the edit service and never writes terrain back. Double-precision and floating-origin from the start.
- **Multiplayer:** a single dynamics writer is already the model. Lazy-generation consensus (who generates an unmaterialized body first) is a later decision (§27).

---

## 23. Frozen vs deferred decisions

**FROZEN in V1** (change = new major version)

1. Ontology: universe → objects → bodies → body artifact; no planet-specific core; capabilities and domains.
2. Identity hierarchy and derivation (§6.3–6.4), including birth-address semantics and `ObjectId` = digest of `UniverseId + address`.
3. Authority table (§3) and the dynamics single-writer rule (§14).
4. Content addressing (§9): BLAKE3, JCS, blob header, codec separation, index entry layout, ledger chain.
5. Frame conventions (right-handed, +Z positive pole), time as `i128` ns, positions as fixed-point µm.
6. `dir_cube/1` and `radial_1d/1` exactly as specified.
7. Reference-surface model: no universal zero; named surfaces.
8. Field descriptor semantics (§12.1); `FieldId` construction; persistence classes.
9. Refinement contract (§13), the C0/C1/C2 law, `cdetail/1` as specified, seed derivation.
10. Contract immutability and the frozen-baseline rule.
11. Lifecycle M0–M6 and the M4 commit protocol.

**DEFERRED** (explicitly not decided now)

- Packed container byte layout; physical sharding of blobs; codec alternatives.
- Galaxy-scale structure and ugen physics beyond `ugen.minimal/1`.
- Concave-shape topology and mesh format.
- Volumetric topology details (`octree_vol`, `layered`).
- Edit operation vocabulary and the sim-state schema.
- Hyperbolic/n-body propagation; non-gravitational force models.
- Capability schemas beyond the V1 eight.
- Multiplayer authority and lazy-generation consensus.
- Migration tooling and lineage format.
- Hemisphere/season labels (UI-level conventions per capability only).

---

## 24. Implementation stages

**Global rules for every stage:** work on branch `stage/NN-slug` (Stage 0 part A commits to `main`); every stage ends with CI green on Windows, Linux and macOS (+ wasm build from Stage 1); every stage adds tests and observable evidence, not prose; no stage implements anything on its "must NOT" list.

---

### Stage 0 — Bootstrap, docs and guardrails

- **Goal:** a repository that enforces the architecture's determinism and review rules from commit one.
- **Scope:** (A) initial commit to `main`: README, `.gitignore`, `.gitattributes` (`* text=auto eol=lf`; binaries explicit), `docs/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md` (this document, supplied by the owner). (B) PR branch: Cargo workspace with empty crates, `rust-toolchain.toml`, `clippy.toml`, `deny.toml`, `rustfmt.toml`, CI matrix, `.coderabbit.yaml`, PR template, `scripts/` skeleton.
- **Repo areas owned:** root files, `docs/`, `.github/`, `crates/*/Cargo.toml` + `lib.rs` stubs, `schema/` (empty with README), `conformance/` (README).
- **Required work:** crate stubs per §17.2 with the dependency direction enforced by `cargo deny` / a workspace test that fails if `veyra-core` gains a forbidden dependency; lint that fails on forbidden float methods, `HashMap` in hashed paths, and `unsafe` in core; CI jobs: fmt, clippy `-D warnings`, test, `cargo build --target wasm32-unknown-unknown -p veyra-core -p veyra-wasm`; `.coderabbit.yaml` with path instructions pointing reviewers at the invariants (no engine types in core, determinism rules, no hashed-file floats, ledger append-only).
- **Hard invariants:** §17.1 determinism rules; dependency direction §17.2; docs are authoritative.
- **Tests:** a deliberately violating fixture proves the lint fails (`tests/lint_fixtures`, run in CI as expected-fail); workspace builds on all four targets.
- **Observable evidence:** green CI on the PR; `main` contains the canonical doc.
- **Done:** PR merged; branch protection recommended (CI required).
- **Must NOT implement:** any format, hashing, or domain logic.
- **Checkpoint:** commit 1 on `main` (docs) + PR #1 (`stage/00-workspace`).

```text
Repository: https://github.com/seth1295/world_system   (public, branch main, currently empty)
Local root: D:\PORT_WORK\Experiment_world_system   (Windows; use PowerShell-safe commands, LF line endings)

TASK: Bootstrap the repository. The canonical architecture document is already saved at
D:\PORT_WORK\Experiment_world_system\docs\VEYRA_WORLD_SYSTEM_ARCHITECTURE.md . If it is missing, stop and report; do not invent it.
Read it fully before doing anything else; it is the implementation authority for this repo. Inspect the live repo and local folder first
(git status, git remote -v, existing files) and adapt; do not assume a clean state.

PART A (direct to main, one commit "docs: canonical architecture"):
- git init if needed; remote origin = the repository above; default branch main.
- Add README.md (project summary in <=25 lines, pointer to the architecture doc, build/test commands), .gitignore (Rust, node, OS files, testdata/, cache/ dirs, target/),
  .gitattributes (* text=auto eol=lf, mark *.zst *.idx *.bin binary), and the architecture doc. Push to origin main.
  Do NOT add a LICENSE file (owner decision pending).

PART B (branch stage/00-workspace, open a PR to main):
- Cargo workspace per doc section 17.2: crates veyra-core, veyra-writer, veyra-universe, veyra-bodygen, veyra-cli, veyra-wasm, veyra-capi, veyra-conformance
  (empty lib/bin stubs with the documented dependency direction; veyra-core has NO dependencies yet and #![forbid(unsafe_code)]).
- rust-toolchain.toml pinned to the exact current stable release number (record it), with the four targets from section 17.1; rustfmt.toml; deny.toml.
- clippy.toml with disallowed-methods per section 17.1 (float sin/cos/tan/asin/acos/atan/atan2/exp/ln/log/pow/powf/powi/mul_add; HashMap/HashSet) applied to veyra-core.
- A test that fails if veyra-core depends on any crate in this workspace other than itself, or on an engine/IO crate.
- Prove the lint works with an expected-fail fixture crate or compile-fail test; CI must run it as an expected failure.
- GitHub Actions: fmt, clippy -D warnings, test on windows-latest/ubuntu-latest/macos-latest, and wasm32 build of veyra-core and veyra-wasm.
- .coderabbit.yaml with path_instructions encoding the architecture invariants (core is sans-IO and engine-free; determinism rules; no JSON floats in hashed files;
  append-only ledgers; consumers never implement world semantics).
- .github/pull_request_template.md (stage, invariants touched, verification output, evidence) ; scripts/ with README only.
- Create empty dirs with README stubs: schema/, conformance/, inspector/, docs/spec/, docs/adr/.

CONSTRAINTS: no format, hashing or domain logic yet. No Unreal. No unsafe in core. Keep files small and reviewable.
VERIFY (paste real output): cargo fmt --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace;
cargo build --target wasm32-unknown-unknown -p veyra-core; the expected-fail lint proof; CI run status link.
COMPLETION REPORT: files created, toolchain version pinned, commands run with results, PR URL, any deviation from the doc with reason.
COMMIT/PUSH: Part A pushed to main; Part B pushed on stage/00-workspace with a PR opened (use gh if available, else give the compare URL).
STOP ONLY IF: the architecture doc is missing, push access fails, or the toolchain cannot be installed.
```

---

### Stage 1 — Canonical foundations: identity, time, JCS, hashing, blobs, ledgers

- **Goal:** all identity and byte-level canonical machinery, byte-exact across targets.
- **Scope:** `ids` (UniverseId, ObjectId, ObjectAddress codec with LEB128/zigzag, text forms), `time` (UTime i128 ns), `canon` (JCS writer and float-rejecting validator, BLAKE3 wrappers, blob header/payload codec, shuffle2, index blob codec, const entries, ledger chain), decimal-string parsing, seed derivation (`body_seed`, `subseed`, `mix64`, `detail_hash`), `veyra` CLI skeleton (`fmt`, `hash`, `id`).
- **Repo areas owned:** `crates/veyra-core/src/{ids,time,canon}`, `veyra-cli`, `conformance/vectors/canon/`.
- **Required work:** golden vectors for JCS, blob bytes, index bytes, ObjectId derivation, seeds; zstd codec in writer-side helper (native) and `ruzstd` decode in core; ledger append/verify (pure functions over bytes).
- **Hard invariants:** §9 exactly; hashed JSON has no non-integer numbers; hashes cover canonical uncompressed bytes; core stays sans-IO.
- **Tests:** published JCS vectors, property tests (encode/decode round trips, shuffle inverse), golden hex for each codec, broken-chain ledger tests, native vs wasm32 test-vector run (Node harness stub OK).
- **Observable evidence:** `veyra hash`, `veyra id object --universe … --address …` print stable outputs equal to committed vectors on all three OSes.
- **Done:** vectors pass natively and under wasm32.
- **Must NOT implement:** topologies, body model, universe logic, sampler.
- **Checkpoint:** PR `stage/01-canon`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first (git pull, read docs/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md sections 6.3, 6.4, 9, 12.4, 13.4, 17). Expected starting state: Stage 0 merged (workspace, CI, lints, empty crates).

GOAL: implement the canonical identity and byte-level foundations in veyra-core, byte-exact on native and wasm32.
SCOPE: modules ids (UniverseId, ObjectAddress kinds 1-4 with LEB128/zigzag canonical encoding, ObjectId derivation, text forms uni:/obj:/bas:), time (UTime i128 ns decimal-string codec),
canon::jcs (RFC 8785 writer; validator rejecting non-integer JSON numbers in hashed documents), canon::hash (BLAKE3, "b3:<hex>"), canon::blob (VYB1 16-byte header + payload, all dtypes, shuffle2, zstd decode via ruzstd; native zstd encode helper outside veyra-core),
canon::index (index blob codec, const entries, key_bytes field), canon::ledger (JSONL hash-chained append/verify as pure byte functions), decimal-string parsing (correctly rounded), seed derivation (body_seed, subseed, mix64, detail_hash per section 13.4).
CLI skeleton in veyra-cli: veyra fmt, veyra hash, veyra id object.
CONSTRAINTS: apply section 17.1 determinism rules (libm only, no forbidden float methods, BTreeMap in hashed paths). veyra-core stays sans-IO, no fs/network, no unsafe. No topology, body model, universe logic or sampler.
Do not hard-code any planet or terrestrial concept anywhere.
VERIFICATION: commit golden vectors under conformance/vectors/canon/ (JCS, blob bytes, index bytes, ObjectId derivations, seed outputs) generated once and then asserted; property tests for round trips;
a negative test per rule (float in hashed JSON, tampered ledger, bad header). CI must run the vector suite on windows, linux, macos and in a wasm32 test runner (wasm-bindgen-test or Node harness). Paste real command output.
COMPLETION REPORT: modules added, vector inventory, test counts, CI links, any spec ambiguity you resolved (and how, to be recorded in docs/adr/), PR URL.
COMMIT/PUSH: branch stage/01-canon, PR to main. STOP ONLY IF a spec rule is internally contradictory or a CI target cannot run the vectors.
```

---

### Stage 2 — Spatial topologies and frames

- **Goal:** the topology abstraction plus both V1 topologies, frozen and independently checkable.
- **Scope:** `topology` trait; `dir_cube/1` (faces, projection, CellKey, tile keys, adjacency, corner rule, metrics, Pos30/LocalPos); `radial_1d/1`; frame math types (`Dir`, axial lat/lon chart, quaternion type), `schema/face_adjacency.toml` + independent derivation script, `cb0-addressing` generator, `veyra cell` and `veyra tile-key` CLI.
- **Repo areas owned:** `veyra-core/src/{topology,frames}`, `schema/face_adjacency.toml`, `scripts/derive_adjacency.py`, `conformance/worlds/cb0-addressing`.
- **Hard invariants:** §10.2–10.3 exactly; sqrt-only warp; cell-centred; no halo storage; solid-angle measure.
- **Tests:** 10⁷ random points round trip; tie cases on axes and diagonals; adjacency involution, table vs derived; neighbour across all 24 edges and 8 corners; ∑area = 4π (within stated tolerance); radial key arithmetic; wasm32 parity on 10⁵ points (byte equality of outputs).
- **Observable evidence:** `veyra cell --dir …` prints CellKey, face, i, j; `veyra tile-key`; `cb0` expected outputs committed.
- **Done:** parity job green.
- **Must NOT implement:** bodies, fields, sampler, refinement.
- **Checkpoint:** PR `stage/02-topology`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read docs/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md sections 10 and 11. Expected starting state: Stage 1 merged (ids, canon, CLI skeleton).

GOAL: implement the Topology abstraction and the two V1 topologies exactly as frozen in section 10, with independent verification.
SCOPE: veyra-core::topology (trait per section 10.1; dir_cube/1 per 10.2 incl. face table, S2 quadratic warp using only sqrt and basic ops, cell-centred (i,j), CellKey u64 Morton encoding, tile keys with tile_log2, adjacency table and corner rule, Pos30/LocalPos, solid-angle cell measure; radial_1d/1 per 10.3 incl. heap CellKey, shell measure, 1-shell clamped halo, linear interpolation stencil);
veyra-core::frames (Dir type, axial lat/lon DISPLAY chart conversions via libm, unit-quaternion type; no seasons, no hemispheres, no "north/south" naming).
schema/face_adjacency.toml plus scripts/derive_adjacency.py that derives the table independently from the face formulas; a Rust test asserting equality and involution.
conformance: generator for cb0-addressing (field storing each cell's own face/i/j; radial key checks) and expected outputs; CLI: veyra cell, veyra tile-key.
CONSTRAINTS: the rest of the system must only depend on the Topology trait, so design the trait for later layered/octree/mesh implementations without adding them. No body model, field storage, sampler, refinement. No planet-specific concepts.
VERIFICATION: 1e7-point random round-trip (point->cell->center->cell), axis/diagonal tie tests, neighbour tests over all 24 edges and 8 corners including flips, area sum 4*pi tolerance documented, radial_1d key/parent/child/measure tests,
native vs wasm32 byte-identical output on 1e5 fixed points (committed query file + expected). Paste real output.
COMPLETION REPORT: public trait surface, test inventory, parity evidence, spec ambiguities recorded in docs/adr/, PR URL.
COMMIT/PUSH: branch stage/02-topology, PR to main. STOP ONLY IF the adjacency table in the doc disagrees with the independent derivation (report both).
```

---

### Stage 3 — Body model, capability schemas, store and loader

- **Goal:** a body artifact can be written, hash-verified and opened with **no terrestrial assumptions** anywhere.
- **Scope:** `model` (body root, classification, physical, figure kinds, frames, reference surfaces, capabilities, domains, field registry, vocab), JSON Schemas under `schema/json/`, `schema/capability_ids.toml`, V1 capability schema documents (all eight), sans-IO `BodyLoader` + `Need` + directory `BlobSource`, `veyra-writer` low-level (write blobs, indexes, sections, `body.json`, `body.id`), `veyra body verify|info|fields`, fixtures `cb5-hashing`, `cb6-compat`, `cb9-minimal-void`.
- **Repo areas owned:** `veyra-core/src/{model,io}`, `veyra-writer`, `schema/`, `conformance/worlds/cb5,cb6,cb9`.
- **Hard invariants:** §4, §9, §11, §12.1 semantics; `critical`/`ancillary` handling; a body with zero capabilities is valid; the core contains no field names such as elevation/ocean.
- **Tests:** JSON Schema validation of every schema doc; unknown critical capability refused; unknown ancillary preserved on rewrite; major+1 refused, minor+1 opened; tamper detection; const and dedup; literal BaselineId for `cb5`; **a source scan test** that fails if core code mentions terrestrial terms outside `schema/capabilities`.
- **Observable evidence:** `veyra body verify cb5` OK; `veyra body info cb9-minimal-void`.
- **Done:** all three fixtures pass on three OSes.
- **Must NOT implement:** sampling of values, pyramid builder, features, refinement, universe.
- **Checkpoint:** PR `stage/03-body-model`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read docs/VEYRA_WORLD_SYSTEM_ARCHITECTURE.md sections 4, 8.2, 9, 11, 12.1, 17.4. Expected starting state: Stages 1-2 merged.

GOAL: define the universal body model and the sans-IO store so bodies can be written, verified and opened with zero terrestrial concepts in the core.
SCOPE: veyra-core::model (body.json root, classification, physical.gm, figure kinds sphere/star_convex_radial/radial_profile_sphere, frames incl. uniform rotation, reference_surfaces sphere/offset_of/figure_surface, capabilities list, domains, FieldDescriptor per 12.1, vocab tables, FieldId=(capability<<16|local)),
JSON Schemas in schema/json/, schema/capability_ids.toml, and the eight V1 capability schema documents in schema/capabilities/ (solid_surface, topography, tectonics, ocean, climate, surface_material, thermal_state, stellar_structure) describing params, field templates, vocab, derived view declarations, requires, display groups (documents only; field data generation is later);
veyra-core::io (Need, BodyLoader::begin/provide/finish, BlobSource trait, hash verification of sections, indexes, blobs; critical vs ancillary handling; required_features check);
veyra-writer low-level API (write sections, blobs, index, body.json, body.id; refuses persistence=dynamic; never rewrites an existing blob);
CLI: veyra body verify, info, fields. Fixtures: cb5-hashing, cb6-compat (all five variants), cb9-minimal-void (zero capabilities, zero fields).
CONSTRAINTS: the core must not contain strings like elevation, sea, ocean, terrain, season, hemisphere outside schema/capabilities documents - add a test that scans source for this. Dynamics descriptor/origin are parsed as opaque hashed sections for now (typed in Stage 8).
No sampling, pyramid builder, features, refinement or universe logic.
VERIFICATION: schema validation of all schema docs, loader tests for every compat variant, tamper test returning the same error code and blob hash from CLI and library, rewrite-preserves-unknown-ancillary test, literal BaselineId assertion for cb5, source-scan test, CI on three OSes + wasm32 build. Paste real output.
COMPLETION REPORT: model structs, error-code table, fixture list with BaselineIds, decisions recorded in docs/adr/, PR URL.
COMMIT/PUSH: branch stage/03-body-model, PR to main. STOP ONLY IF the spec contradicts itself on identity or compat rules.
```

---

### Stage 4 — Sampler, derived views and the non-terrestrial fixtures

- **Goal:** canonical values can be read at any position, and the first two non-terrestrial bodies prove the model.
- **Scope:** decode, pyramid builder in the writer, `sample`, `tile` with halos (cross-face), bilinear and nearest, time slices and reductions, nodata, `histogram`/`stats` (measure-weighted), `views()` catalog, derived builtins (`cube_face`, `tile_level`, `axial_latitude`, `slope`, `land_ocean` gated by capability), `domain_geometry` for sphere and star_convex figures, `inspect`; fixtures `cb1-seams`, `cb2-categories`, **`cb7-star1d`**, **`cb8-rock`**; CLI `veyra body sample|tile|inspect|views`.
- **Repo areas owned:** `veyra-core/src/{sample,views}`, writer pyramid, conformance fixtures + queries + expected.
- **Hard invariants:** §10, §12.1; stats weighted by domain measure; sampling uses only the Topology trait; derived views are core-implemented and capability-gated.
- **Tests:** analytic function recovery; continuity across all edges and corners; categorical ties; nodata; `cb7` samples radial profile with no figure surface; `cb8` produces non-spherical geometry from radius field; views list for `cb7`/`cb8`/`cb9` contains no terrestrial groups; native vs wasm byte parity on all queries.
- **Observable evidence:** `veyra body views cb7-star1d` lists only stellar views; `veyra body sample cb8-rock …`.
- **Done:** fixtures pass bit-exact, parity green.
- **Must NOT implement:** refinement (`above_native` beyond `smooth_only/inherit/none`), features, validator, universe.
- **Checkpoint:** PR `stage/04-sampler`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc sections 10, 12.1, 12.4, 17.4, 18.1 (cb1,cb2,cb7,cb8,cb9). Expected starting state: Stages 1-3 merged.

GOAL: implement the canonical sampler over the Topology trait and prove it on terrestrial-free fixtures.
SCOPE: veyra-core::sample (raw decode with scale/offset, nodata, nearest and bilinear/linear stencils from the topology, cross-face halos, periodic slices and reductions mean/min/max, tile(), histogram()/stats() weighted by Topology::cell_measure, source reporting Stored/Pyramid/Const/Nodata/Inherited),
veyra-writer pyramid builder (integer downsample operators per 12.1; mean = floor((a+b+c+d+2)/4)), veyra-core::views (ViewDescriptor catalog generated from fields + capability-declared derived views + topology-derived views; builtins cube_face, tile_level, axial_latitude (15 deg bands), slope, land_ocean only when ocean+topography declared),
Body::domain_geometry for figure sphere and star_convex_radial, Body::inspect. Fixtures: cb1-seams, cb2-categories, cb7-star1d (no solid surface, no datum, radial_1d only, stellar_structure fields), cb8-rock (star-convex irregular figure, no height datum/ocean/climate/orbit).
Add queries/expected (f64 as hex bits) and a Node harness stub that will become the wasm parity runner. CLI: veyra body sample, tile, inspect, views.
CONSTRAINTS: no refinement algorithm yet (above_native refine must return a clear Unsupported error), no features, validator or universe. The sampler MUST NOT special-case any capability or field name. Do not add terrestrial defaults.
VERIFICATION: analytic recovery tests; continuity across all 24 edges and 8 corners; categorical tie/nodata cases; views for cb7/cb8/cb9 contain no Topography/Tectonics/Ocean/Climate groups (test); area-weighted stats sum to total measure; native vs wasm32 byte-identical outputs for all committed queries. Paste real output.
COMPLETION REPORT: API surface added, fixture inventory with BaselineIds, parity evidence, ADRs, PR URL.
COMMIT/PUSH: branch stage/04-sampler, PR to main. STOP ONLY IF cross-face sampling cannot reach the required determinism (report the failing case).
```

---

### Stage 5 — Features and provenance

- **Goal:** discrete entities, cell↔feature links and "why is this here?" chains.
- **Scope:** feature table codec (`jcs-json`), ordinal/stable_id derivation (§6.4), geometry (`polyline_udeg`), `feature_ref` sampling and resolution, DAG and explain recipes (capability-contributed), `explain()`, fixture `cb3-features`, CLI `veyra body explain`.
- **Repo areas owned:** `veyra-core/src/{features,provenance}`, writer feature support, `conformance/worlds/cb3-features`, tectonics capability explain recipe.
- **Hard invariants:** §12.3, §16.1; ordinals from sorting stable IDs; stable IDs independent of generation order; explain recipes are data.
- **Tests:** ordinal determinism under permuted input; referential integrity; recipe resolution; explain chain for cb3 (cell → plate → boundary → plates); a stellar explain recipe fixture on cb7 proving recipes are capability-contributed.
- **Observable evidence:** `veyra body explain cb3 …` prints the chain and template text.
- **Done:** cb3 + star recipe pass, parity green.
- **Must NOT implement:** columnar tables, graph geometry, explain re-evaluation mode.
- **Checkpoint:** PR `stage/05-features-provenance`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc sections 6.4, 12.3, 16.1, 18.1 (cb3). Expected starting state: Stages 1-4 merged.

GOAL: implement feature tables, feature_ref resolution and data-driven explain chains.
SCOPE: veyra-core::features (jcs-json tables, typed columns u32/u64hex/vocab/ref/dec/polyline_udeg, stable_id = blake3.derive_key("veyra.feat.v1", object_id||table||structural_key)[..8], dense 1-based ordinals by ascending stable_id, geometry to unit vectors, feature_ref sampling via the sampler),
veyra-core::provenance (DAG model, explain recipes read from provenance/explain.json, Body::explain returning a structured chain plus template text, honest "contributions, not counterfactuals" metadata),
writer support for feature tables, DAG and explain docs; tectonics capability schema gains its explain recipe; stellar_structure capability gets a small recipe so the machinery is proven off-terrestrial.
Fixture cb3-features (3 plates, 4 boundaries, nearest_boundary raster) and cb7 explain test. CLI: veyra body explain.
CONSTRAINTS: no columnar tables, graph/volume geometry, re-evaluating explain mode, validator, universe. Explain logic must not special-case capability names; everything comes from recipe data.
VERIFICATION: permutation test for ordinals, referential integrity, recipe evaluation tests, cb3 expected explain output, cb7 recipe output, native vs wasm byte parity. Paste real output.
COMPLETION REPORT: what was added, fixture IDs, parity evidence, PR URL.
COMMIT/PUSH: branch stage/05-features-provenance, PR to main. STOP ONLY IF stable_id derivation proves order-dependent in the spec as written.
```

---

### Stage 6 — Validator

- **Goal:** one authoritative rule set that any producer or consumer can run.
- **Scope:** `validate` fast/standard/full per §18.3, JSON/human report, `--strict`, `--write-diag`, `veyra body diff`, one failing fixture per rule, class-profile checks, capability-consistency checks, forbidden-term scan for core schemas.
- **Repo areas owned:** `veyra-core/src/validate`, CLI, `conformance/broken/`.
- **Hard invariants:** validator never trusts a producer: it recomputes pyramids, hashes, classifications.
- **Tests:** all existing fixtures pass `full` except where refinement/dynamics rules are not yet applicable (those rules are registered but skipped with a "not applicable" status); each rule has a minimal breaking fixture and a stable error code.
- **Observable evidence:** `veyra body validate <fixture> --json`.
- **Done:** every rule has pass and fail evidence.
- **Must NOT implement:** refinement self-check logic or dynamics checks (registered stubs only).
- **Checkpoint:** PR `stage/06-validator`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc section 18.2-18.3. Expected starting state: Stages 1-5 merged.

GOAL: implement the validator (fast/standard/full) as a library in veyra-core::validate plus the CLI front end.
SCOPE: all rules in 18.3 that apply to what exists (schema validity, no JSON floats, hash chains, required_features, capability requires/consistency, ID allocation, references, reference-surface graph acyclic, blob/index integrity, tile coverage, const legality, dtype/nodata ranges, ordinals, component identities when declared, roughness/height level equality, persistence != dynamic, class profile, ledger chains, pyramid recomputation, seam statistics, DAG acyclic, explain resolvability, determinism rebuild check).
Rules for refinement self-check, dynamics consistency, sketch_hash and derived-index agreement are registered with stable codes and report "not-applicable" until their stages land.
Stable error codes and a JSON report schema (schema/json/validation_report.json). Exit codes per 18.2. veyra body diff (manifest-level). --write-diag writes diagnostics/validation.json.
conformance/broken/: one minimal failing fixture per rule (generated by a script from good fixtures).
CONSTRAINTS: validator recomputes, never trusts. No refinement or dynamics logic. No universe code.
VERIFICATION: every good fixture passes full; every broken fixture fails with exactly its expected code; JSON report validated against its schema; native vs wasm run of fast/standard on the corpus (wasm may skip native-only IO via the BlobSource trait). Paste real output.
COMPLETION REPORT: rule table (code, level, fixture), results, PR URL.
COMMIT/PUSH: branch stage/06-validator, PR to main. STOP ONLY IF a rule cannot be decided from stored data.
```

---

### Stage 7 — Normative refinement

- **Goal:** canonical sub-resolution authority implemented once, pinned by vectors.
- **Scope:** `refine` fixed-point lib, seeds, `cdetail/1`, `inherit`, `threshold_partition` guard, `Canonical` level sampling, refined `tile()`, `refine_vectors`, validator refinement checks, fixture `cb4-refine`.
- **Repo areas owned:** `veyra-core/src/refine`, `conformance/refine_vectors`, validator additions.
- **Hard invariants:** §13 exactly: integer-only, exact mean conservation, order independence, guard output clamping, no feature motion.
- **Tests:** property tests (Σchildren = 4p to depth 8 for random fields), ZeroSum4, order-independence (query permutations), cross-face roughness interpolation, guard counters, guard_rate = 0 on cb4, pinned BLAKE3 of tiles at levels 6/9/12, wasm byte parity.
- **Observable evidence:** `veyra body tile cb4 --level 12 …` and the pinned hashes.
- **Done:** vectors committed and enforced.
- **Must NOT implement:** `post_steps`, other algorithms, material refinement.
- **Checkpoint:** PR `stage/07-refinement`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc section 13 in full. Expected starting state: Stages 1-6 merged.

GOAL: implement veyra.refine.cdetail/1, inherit, and the threshold_partition guard EXACTLY as specified; make them part of sampler, tile(), and the validator.
SCOPE: refine::{fixed,seed,cdetail,guards,inherit}; LevelSel::Canonical; plan() for refined tiles (native tiles + halo ring); refined tile cache (RAM only, never authoritative); validator refinement checks (mean conservation, guard_rate, I-R1, E_REFINE_RANGE);
fixture cb4-refine (native level-3 height with a threshold-partition boundary and a land interior, plus roughness), conformance/refine_vectors with pinned BLAKE3 of canonical refined tiles at levels 6, 9, 12.
CONSTRAINTS: integer arithmetic only on i64 in working units 2^-10; no floats in C0. Algorithm semantics are immutable: if you believe section 13.5 has an error, STOP and report with a concrete counterexample rather than changing it. No post_steps, other algorithms, or material refinement.
Algorithm must not depend on traversal order, thread, cache or request pattern (test with permuted/parallel queries).
VERIFICATION: property tests of exact mean conservation to depth 8 over random fields, ZeroSum4 sums to 0, guard clamp behaviour, cross-face roughness interpolation continuity, pinned vectors, native vs wasm32 byte-identical refined tiles. Paste real output and the pinned hashes.
COMPLETION REPORT: algorithm test inventory, vector list, performance note (ms per 128x128 tile at depth 10), PR URL.
COMMIT/PUSH: branch stage/07-refinement, PR to main. STOP ONLY IF the spec algorithm provably fails mean conservation or determinism (give the counterexample).
```

---

### Stage 8 — Dynamics and the universe core (M0–M3)

- **Goal:** stable identity before materialization, the lifecycle through orbital resolution, and the single-writer dynamics contract.
- **Scope:** `dynamics` (DynState, keyframes, ledger, `kepler2` elliptic propagation, classification, `commit_keyframe`, recipe_backcast flag), typed dynamics descriptor/origin in the body model, `veyra-universe` (universe.json, UniverseId, lattice index, `ugen.minimal/1`, ObjectAddress/ObjectId assignment, M1–M3 registry ledger, system birth manifests, relations ledger, derived index, status queries), CLI `veyra universe init|info|resolve|status|list|verify|repair`, `veyra body dynamics`, fixture `cu0-universe`.
- **Repo areas owned:** `veyra-core/src/dynamics`, `crates/veyra-universe`, `conformance/worlds/cu0-universe`.
- **Hard invariants:** §3, §6, §7 (M0–M3), §14.1–14.2: one writer, no duplicate position in the index, ObjectId independent of generator build, fixed-point states, recipe sketches deterministic and non-persistent.
- **Tests:** ObjectId vectors; address codec; ledger replay and broken-chain detection; crash simulation (partial writes); two-run and cross-OS identical resolution; Kepler propagation vs analytic tests (period closure, energy conservation); an injected duplicate state in the derived index is detected by `verify`; reference change keeps ObjectId.
- **Observable evidence:** `veyra universe resolve … --to orbital` prints systems and bodies with stable IDs; `ls` shows zero body artifacts.
- **Done:** all of the above pass on three OSes.
- **Must NOT implement:** body generation, materialization, universe browsing in the Inspector, hyperbolic propagation.
- **Checkpoint:** PR `stage/08-dynamics-universe`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc sections 3, 6, 7, 8.1, 14, 15 and 18.1 (cu0). Expected starting state: Stages 1-7 merged.

GOAL: implement universe identity, the M0-M3 lifecycle with zero body storage, and the dynamics authority contract.
SCOPE: veyra-core::dynamics (DynState with i128 um positions / i64 um/s velocities, Keyframe, hash-chained dynamics ledger, veyra.prop.kepler2/1 elliptic two-body propagation in f64+libm, classification recomputation, exact reference-change re-expression, recipe_backcast flag, commit_keyframe as the ONLY append path); type the body dynamics descriptor/origin sections;
crate veyra-universe: universe.json + UniverseId, veyra.uidx.lattice/1 (RegionKey, i64 lattice), recipe contract veyra.ugen.minimal/1 (deterministic, integer-seeded: systems per seeding cell, one star + a few bodies per system incl. classes terrestrial.habitable_test/1, stellar.main_sequence_lite/1, plus procedural-only classes (gas giant, comet) that can be resolved but not materialized),
ObjectAddress -> ObjectId, registry ledger (M1-M3 transitions), systems/<id>.json birth manifests, relations ledger, derived index (rebuildable), sketches as cache-only values; materialization hooks exist as an unimplemented trait.
CLI: veyra universe init|info|resolve|status|list|verify|repair, veyra body dynamics. Fixture cu0-universe.
CONSTRAINTS: no body generation or materialization, no Inspector, no hyperbolic/n-body. Universe state never stores positions of materialized bodies (derived index only). Systems are not folders owning bodies.
VERIFICATION: ObjectId and address vectors; ledger replay and tamper detection; crash tests (truncated line, missing rename); two runs and three OSes produce identical resolution output; Kepler tests (period closure, energy); duplicate-position injection detected by `universe verify`; reference-change keeps ObjectId. Paste real output.
COMPLETION REPORT: contract summary, vector list, cross-OS evidence, ADRs, PR URL.
COMMIT/PUSH: branch stage/08-dynamics-universe, PR to main. STOP ONLY IF the single-writer/derived-index contract cannot be made consistent (explain precisely why).
```

---

### Stage 9 — Materialization and body generators

- **Goal:** the first real bodies exist as persistent artifacts, committed atomically, and are never regenerated.
- **Scope:** `veyra-bodygen`: `terrestrial_test/1` and `star_lite/1`; writer integration (pyramid, components, features, DAG, explain, diagnostics with stage snapshots); universe materializer (staging, validate, atomic rename, ledger commit, orphan handling); `veyra universe materialize`; `veyra body export`; `VEYRA_GEN_PERTURB` test switch; sketch constraints and `sketch_hash`.
- **Repo areas owned:** `crates/veyra-bodygen`, `veyra-universe` materializer, `veyra-writer` builders, CLI.
- **Generator content (terrestrial_test):** Voronoi plates on the sphere with Euler poles and velocities; boundaries classified by relative motion; crust type/age; height = isostatic + tectonic (boundary-distance kernels) + surface-process (deterministic noise); roughness; dominant process; placeholder temperature/precipitation from axial insolation and height with periodic slices tied to the dynamics origin. Stage names in the DAG say what they are. `star_lite`: analytic radial profile.
- **Hard invariants:** §7 commit protocol; frozen baseline; identity independent of generator build; component identity exact; guard_rate < 1e-4; generators depend on `veyra-writer` only.
- **Tests:** materialize T and A (Auren); `validate --level full`; same universe and address give the same `BaselineId` in the pinned environment; **regeneration refusal** and perturbation test; crash tests (kill between rename and ledger, between validate and rename); export opens with universe hidden; baseline excludes state, meta and diagnostics.
- **Observable evidence:** the ledger record, `body.id`, and `ls materialized/`.
- **Done:** items 1–4, 11, 12 of §21 pass via CLI.
- **Must NOT implement:** edits, sim, packed formats, further body classes.
- **Checkpoint:** PR `stage/09-materialization`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc sections 7, 8, 9, 14.1, 15, 16, 20, and 21 items 1-4, 11, 12. Expected starting state: Stages 1-8 merged.

GOAL: materialize real bodies from the procedural universe into persistent, validated, atomically committed artifacts, and prove they are never regenerated.
SCOPE: crate veyra-bodygen with terrestrial_test/1 (profile: radius 6.0e6 m, native height L9, climate L7, tile_log2=7; Voronoi plates with Euler poles/velocities, boundaries classified by relative motion, crust type/age, height = isostatic + tectonic + surface-process with the exact component identity, roughness, dominant_process, placeholder temperature/precipitation with periodic slices tied to the dynamics origin; DAG with HONEST stage names; explain recipes; diagnostics with stage snapshots using the shared blob store)
and star_lite/1 (analytic radial profile, stellar_structure capability, photosphere reference surface). Generators depend only on veyra-writer and receive the sketch as hard constraints; baseline records origin.sketch_hash; dynamics origin keyframe = recipe evaluated at materialization time.
veyra-universe materializer: stage into materialized/.staging, run validate standard, compute BaselineId, atomic rename into materialized/obj_<hex>.veyra/, append the registry ledger 'materialized' record (commit point), refuse if already materialized, orphan/missing handling in `universe repair`.
CLI: veyra universe materialize, veyra body export (self-contained closure). Test-only env switch VEYRA_GEN_PERTURB=1 changes generator output so tests can prove no regeneration.
CONSTRAINTS: no edits/sim/packed formats/extra body classes. Generators may use libm and f64 but the artifact is the authority; nothing in baseline may depend on host, time or paths. Do not duplicate world semantics from veyra-core inside bodygen (use writer/core APIs for cell keys, pyramids, quantization).
VERIFICATION: materialize T and Auren; `veyra body validate --level full` exits 0 (guard_rate < 1e-4, exact component identity); same universe + address => same BaselineId across two runs in the pinned environment; PERTURB run is refused and body.id unchanged; kill-injection tests at each commit step; export opens with universe/store hidden and has the same baseline_id; baseline excludes state/meta/diagnostics/cache. Paste real output.
COMPLETION REPORT: generator stage list (honestly labelled placeholders), BaselineIds, timings, validation outputs, PR URL.
COMMIT/PUSH: branch stage/09-materialization, PR to main. STOP ONLY IF atomic commit semantics cannot be achieved on Windows (report the filesystem behaviour found).
```

---

### Stage 10 — WASM bindings, C ABI shell and parity harness

- **Goal:** the same core in a browser worker and Node, with CI-enforced bit parity.
- **Scope:** `veyra-wasm` (wasm-bindgen over the API in §17.4, typed-array transfer, no logic), Node harness, parity runner over all corpus queries and materialized bodies (including `Canonical` heights and `state(t)`), `veyra-capi` shell (handles, error codes, header via cbindgen, smoke test), bundle size budget.
- **Repo areas owned:** `veyra-wasm`, `veyra-capi`, `conformance/parity`, CI.
- **Hard invariants:** §13.7 bit-identity; no semantics in bindings.
- **Tests:** byte-identical outputs native vs WASM for all queries; C ABI loads a body and samples through a C test program; size < 1.5 MB gzipped.
- **Observable evidence:** a parity report artifact in CI.
- **Done:** parity job green on three OSes.
- **Must NOT implement:** Inspector, Unreal adapter.
- **Checkpoint:** PR `stage/10-wasm-parity`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc sections 13.7, 17, 19.3, 21 item 10. Expected starting state: Stages 1-9 merged.

GOAL: expose the existing core through WASM and a C ABI without adding any semantics, and enforce native/WASM bit parity in CI.
SCOPE: crates/veyra-wasm (wasm-bindgen API mirroring section 17.4: open/loader steps, plan/provide, sample, tile, domain_geometry, views, stats/histogram, inspect, explain, features, dynamics state(t), validate-fast; typed-array transfer; npm package layout under packages/veyra-wasm);
Node harness executing every committed query file (conformance/queries) and a parity set generated from materialized bodies (>=200 fixed points over six faces, seams, Canonical-level refined heights, state(t) for both bodies), emitting f64 as hex bits;
crates/veyra-capi (opaque handles, stable numeric error codes, missing-need out-list, cbindgen header, a C test program that opens a body and samples);
CI job comparing native CLI output with Node output byte-for-byte and uploading a parity report.
CONSTRAINTS: bindings contain no world logic; no Inspector; no Unreal. Keep gzip size of the wasm under 1.5 MB (report the actual number).
VERIFICATION: parity job green on windows/linux/macos; C smoke test passes; size report. Paste real output.
COMPLETION REPORT: API list, parity statistics, bundle sizes, PR URL.
COMMIT/PUSH: branch stage/10-wasm-parity, PR to main. STOP ONLY IF native and WASM disagree in a way that implicates the spec (give the minimal failing query).
```

---

### Stage 11 — Inspector shell (body-aware)

- **Goal:** open any body, see its real data, switch views generated from its capabilities.
- **Scope:** Vite + TS + three.js app, worker hosting WASM, loader fetch loop, surface_mesh view (quadtree LOD, tile geometry from the core, texture + LUT shading), radial_profile view, ViewDescriptor-driven dropdown with cube icon, dynamic legend (colour coding, categories with weighted counts, stats), top-right card, orbit/zoom, error card. Playwright smoke tests.
- **Repo areas owned:** `inspector/`.
- **Hard invariants:** §19.3 rule: no world semantics in TypeScript; views come only from `views()`; no terrestrial groups unless declared.
- **Tests:** Playwright: for T, A, `cb7`, `cb8`, `cb9`, enumerate dropdown entries (match core `views()` exactly), select each, assert non-blank canvas, no console errors, legend counts equal CLI histograms; tampered blob error card.
- **Observable evidence:** screenshots per view saved as CI artifacts.
- **Done:** all of the above on Chromium.
- **Must NOT implement:** point inspect/explain, overlays, diagnostics, compare, universe browsing.
- **Checkpoint:** PR `stage/11-inspector-shell`.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc section 19 in full. Expected starting state: Stages 1-10 merged (veyra-wasm package builds, materialized bodies for a test universe can be produced by CLI).

GOAL: a body-aware browser Inspector shell that only displays what the core returns.
SCOPE: inspector/ (Vite, TypeScript, three.js WebGL2, no UI framework, <=150 lines CSS): a Worker hosting veyra-wasm; fetch loop implementing the sans-IO need/provide protocol over plain static HTTP (6 parallel, hash-verified by the core); ?body=<url> opens one body artifact;
views dropdown (cube icon + Debug button) built ONLY from Body.views() grouped by display.group; right-side compact info card with mode name, description, colour legend, category swatches with measure-weighted counts, stats; orbit/zoom;
surface_mesh view for dir_cube domains (per-face quadtree LOD, 33x33 tile meshes from Body.domain_geometry, view tile as R32F halo-1 texture + 256x1 LUT shader, vocabulary colours via NEAREST), radial_profile view for radial_1d domains (2D cutaway disc + line profile in the card); exaggeration slider only where the body declares a height-like field; visible error card with validator error code on failure.
Dev server script that serves a body directory with plain static HTTP. Playwright tests.
CONSTRAINTS: TypeScript MUST NOT compute cell keys, projections, interpolation, derived layers, refinement, feature resolution, propagation or class logic. No terrestrial strings in inspector source (scan test). No point inspect/explain/overlays/diagnostics/compare/universe browsing yet; no design system.
VERIFICATION: Playwright over body T, star A, cb7-star1d, cb8-rock, cb9-minimal-void: dropdown entries equal core views() exactly; each view renders non-blank with no console errors; legend category counts equal `veyra body` histograms; tampered blob shows error card with the same code as `veyra body verify`. Save per-view screenshots as CI artifacts. Paste real output.
COMPLETION REPORT: screenshots index, view inventory per body, bundle size, PR URL.
COMMIT/PUSH: branch stage/11-inspector-shell, PR to main. STOP ONLY IF a needed capability is missing from the WASM API (list exactly which).
```

---

### Stage 12 — Point inspection, provenance, overlays, diagnostics and final acceptance

- **Goal:** complete the V1 Inspector and run the full §21 acceptance.
- **Scope:** pick → worker `inspect`, Selected-point block (source and level per field, dynamics summary), explain card, feature overlay, diagnostics group and Stage select, scripted acceptance (`scripts/acceptance.*`), docs: `docs/spec/` extracted from this document where useful, ADR index, README quick start.
- **Repo areas owned:** `inspector/`, `scripts/`, `docs/`.
- **Hard invariants:** inspect reads canonical data, never pixels; explain is displayed as returned.
- **Tests:** the full §21 acceptance runs in CI; fixed-point pick returns the expected values; overlay segment count equals table vertices; stage `final` equals canonical byte-for-byte.
- **Observable evidence:** the acceptance run log and screenshots.
- **Done:** all 15 acceptance items pass.
- **Must NOT implement:** compare mode, universe browsing, edits.
- **Checkpoint:** PR `stage/12-inspector-complete`; tag `v0.1.0` after merge.

```text
Repository: https://github.com/seth1295/world_system
Local root: D:\PORT_WORK\Experiment_world_system
Inspect the live repo first. Read doc sections 16, 19, 21 (all 15 acceptance items). Expected starting state: Stages 1-11 merged.

GOAL: finish the V1 Inspector and make the full first end-to-end acceptance an automated, repeatable script.
SCOPE: click/pick -> worker inspect(pos) on canonical data (never pixels); Selected-point block listing every field in the domain with source (stored/pyramid/refined/inherited/const/nodata) and level used; dynamics summary (class, reference, state(t)); explain card rendering the core's chain + template; feature overlay (any feature table with polyline geometry, coloured by vocab; toggle in toolbar);
diagnostics group + Stage select when diagnostics/diag.json matches baseline_id; scripts/acceptance.ps1 and acceptance.sh implementing section 21 items 1-15 end to end (universe init, resolve, materialize, validate, no-regeneration test, serve, Playwright, parity, export-with-hidden-universe, cache-deletion, tamper test, derived-index test, fixture-purity scan);
README quick start; docs/adr index; extract any useful normative parts of the architecture doc into docs/spec/ WITHOUT altering the architecture doc's meaning.
CONSTRAINTS: no compare mode, no universe browsing in the Inspector, no edits. TypeScript still contains no world semantics.
VERIFICATION: run the full acceptance script in CI on linux and locally on Windows; attach logs and screenshots; each of the 15 items reported PASS/FAIL with the command that proved it. Paste real output.
COMPLETION REPORT: acceptance table with evidence, known limitations, follow-up list, PR URL.
COMMIT/PUSH: branch stage/12-inspector-complete, PR to main; after merge create tag v0.1.0. STOP ONLY IF an acceptance item cannot pass for a reason traceable to the spec (give the minimal reproduction).
```

---

## 25. Agent prompts

The paste-ready prompt for each stage is included at the end of that stage in §24.

---

## 26. PR and review workflow

```
stage branch (stage/NN-slug) → implement → local verification (fmt, clippy -D warnings, tests, stage-specific evidence)
→ push → PR to main (template fills stage, invariants, evidence) → CI (3 OSes + wasm) + CodeRabbit review
→ address review (fix, or reply with reasoning citing the architecture document) → squash-merge → delete branch
```

- **Merge rule:** CI green, and every CodeRabbit comment either fixed or answered with a reason. A comment that identifies a **violated invariant** blocks the merge. Style comments do not.
- **One PR per stage** (13 PRs). Stages 1 and 2 may share a PR if the author prefers, since both are foundations with no external behaviour. Stages 5 and 6 may share one, but keeping them separate keeps the validator's review focused. **Do not merge** 3 with 4, 8 with 9, or 10 with 11: each is a distinct authority boundary.
- **Architecture changes:** any change to a FROZEN item (§23) needs an ADR in `docs/adr/` **and** an edit to this document in the same PR, with a version bump (`format_version`) where the format is affected. Non-frozen clarifications are ADR-only.
- **Tags:** `v0.1.0` after Stage 12.
- No release process, no additional bureaucracy. Branch protection on `main` requires CI only.

---

## 27. Product-owner decisions

Only these require you. Everything else in this document is decided.

1. **License.** The repository is public; with no license file, nobody may legally reuse it. Choose a license, or choose to keep "all rights reserved" explicitly. Stage 0 deliberately omits one.
2. **Frozen-baseline rule.** Do you accept that a materialized body and every save bound to it is permanent? A better generator makes new bodies or a new universe, and rebasing edits onto regenerated worlds is unsupported. This affects every design choice here. Confirm or reject.
3. **Concave and contact-binary bodies.** V1 represents irregular bodies only as star-convex shapes. If overhanging or concave asteroids and comets are required for gameplay in the near term, the mesh topology moves up. Decide the priority. It does not block V1.
