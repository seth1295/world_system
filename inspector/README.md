# VEYRA Inspector prototype

This is a browser-only, mock-data prototype of the VEYRA body Inspector. It is intentionally isolated under `inspector/` and is not a Rust-core implementation.

## Run it

```sh
npm install
npm run dev
```

Open the local URL printed by Vite. For checks, run `npm run build`, `npm run lint`, `npm test`, and `npm run test:e2e`. The browser smoke test uses the environment's Chromium executable (`CHROMIUM_PATH` can override `/usr/bin/chromium`) and saves four body/view captures plus a selected-point capture under `screenshots/`.

## Structure

- `src/domain.ts` defines the consumer contracts and display-only response types.
- `src/mock-provider.ts` supplies three mock body summaries, capability-style view descriptors, representative query results, and presentation geometry/tiles.
- `src/ui/app.ts` builds the toolbar, descriptor-driven Debug menu, info/legend card, point inspector, and status strip. It depends on `BodyCatalog` and `BodyProvider`, not on body-specific data.
- `src/render/viewport.ts` consumes provider geometry and tiles, maps view palettes for display, renders with three.js/WebGL2, and sends picked surface directions or radii back through the provider.

The app keeps one large interactive viewport. Veyra and Irregular Rock use a lit surface presentation; Auren uses a radial cutaway disc and an inline radial profile chart. Descriptors returned by each provider determine which Debug groups and views exist.

## Provider boundary

`BodyCatalog` discovers and opens a body. A `BodyProvider` returns `summary()`, `views()`, `stats()`, `domainGeometry()`, `tile()`, `radialProfile()`, `inspect()`, `explain()`, `features()`, and `diagnostics()`. The UI does not contain a terrestrial menu or body-class switch. The intended seam is `MockBodyProvider → WasmBodyProvider` (and `MockBodyCatalog → WasmBodyCatalog`): the WASM adapter returns the same descriptors, typed arrays, and query reports to the same UI and renderer. The Rust/WASM core remains the only authority for canonical data and semantics.

## Explicit prototype limits

All displayed bodies, views, statistics, point values, overlays, profile values, and render geometry are mock presentation data. This prototype does **not** implement CellKey logic, dir_cube projection, sampling, interpolation, refinement, field semantics, generation, body classification, orbital propagation, feature resolution, canonical parsing, `.veyra` loading, or any other world-system semantics. It does not depend on or copy the unmerged `phase/01-core-foundation` branch. It currently has no worker, static artifact loader, real WASM adapter, diagnostics snapshots, or canonical error handling.
