# VEYRA Inspector adaptability prototype

This is a browser-only information-architecture and adaptability stress prototype. Its bodies, descriptors, reports, failures, and display geometry are deterministic **synthetic fixtures**, not canonical VEYRA worlds. A persistent `SYNTHETIC FIXTURE` label and `fixture:` object IDs make that status visible.

The prototype asks whether the Inspector can display the information returned by different provider descriptors: from an empty catalogue to 72 views, 53 time choices, 36 legend categories, 46 point fields, deep explanations, multiple domains, diagnostics, overlays, partial responses, and explicit load failures. The models use neutral presentation geometry; they are not concept art or physically meaningful world data.

## Run it

```sh
npm ci
npm run dev
```

Vite prints the local browser URL. Select a fixture in the bottom fixture control, or open a reproducible scenario directly, for example `/?fixture=fixture%3Aview-catalog-extreme`.

## Checks and captures

```sh
npm run typecheck
npm run lint
npm test
npx playwright install chromium
npm run test:e2e
npm run screenshots
```

Playwright uses its managed Chromium when `CHROMIUM_PATH` is unset or blank. If browser downloads are blocked in an environment, set `CHROMIUM_PATH` to an installed Chromium executable for that run. The config has a unit test that confirms no executable path is set when the override is absent. E2E screenshots for the required viewport/scenario matrix are written to `screenshots/stress/` as JPEGs.

## Scenario fixtures

Scenario recipes live in `src/fixtures/scenarios.ts`. The typed builders expand a seed and a small set of parameters into domains, descriptor groups, views, legends, time selections, point reports, explanation chains, diagnostic stages, feature tables, display geometry, and scripted provider behavior. The same recipe and seed produce the same responses. To add a scenario, compose a `FixtureSpec`, add it to `SCENARIOS`, and use the existing builders; do not add UI cases or body-specific branches.

Named scenarios include:

- `void`, `minimal`, `normal-surface`, `radial`, `irregular`
- `view-catalog-extreme`, `temporal-heavy`, `category-heavy`
- `point-heavy`, `provenance-heavy`, `multi-domain`
- `diagnostics`, `diagnostics-single`, `diagnostics-unavailable`
- `features`, `features-no-geometry`, `features-none`
- `loading`, `view-loading`, `partial-data`, `missing-content`
- `validation-failure`, `unsupported-critical`, `retryable-error`, `non-retryable-error`

The `fixture:` selector is a prototype-only control separate from the Inspector toolbar. It also updates the URL query so tests and captures can reproduce a state.

## Provider and rendering boundary

The UI receives a `BodyCatalog` and depends on the `BodyProvider` interface in `src/provider/contracts.ts`. `MockBodyCatalog` and `MockBodyProvider` implement those interfaces using only `src/fixtures/` data. The UI requests summaries, domains, ordered view descriptors, stats, geometry, tiles, point inspection, explanations, feature geometry, and diagnostics through that provider. It formats returned values, draws the returned model/tile, runs camera controls, filters descriptors, and exposes provider status.

The intended adapter path is:

```text
Inspector UI → BodyProvider → MockBodyProvider (today)
                         ↘ WasmBodyProvider (future)
```

`WasmBodyProvider` is not implemented here. It should call `veyra-wasm` for body queries and return core-produced descriptors, values, reports, geometry, feature responses, diagnostics, and failures through the same contract. Fixture construction remains replaceable without changing the UI.

The UI and renderer cannot import fixture construction or `MockBodyProvider`; ESLint and a unit import-boundary test enforce this. A separate architecture test scans every other source file under `src/` for forbidden semantic vocabulary. The vocabulary list lives in `tests/architecture-guard.test.ts`.

## Explicit limits

This prototype does not implement CellKey or address generation, topology projection, sampling, interpolation, refinement, field semantics or derived-field calculation, feature resolution, propagation, body-class interpretation, artifact or `.veyra` parsing, canonical validation, or generation of worlds. It has no Rust or WASM integration and does not consume the unmerged `phase/01-core-foundation` branch. Procedural values and geometry exist only as presentation fixtures and must not be treated as VEYRA data.

Provider-shape gaps found while implementing the display contract are listed in [`docs/PROVIDER_CONTRACT_FINDINGS.md`](docs/PROVIDER_CONTRACT_FINDINGS.md).
