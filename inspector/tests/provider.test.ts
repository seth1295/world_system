import { describe, expect, it } from 'vitest';
import { buildFixture, buildGeometry, SCENARIOS } from '../src/fixtures/scenarios';
import { MockBodyCatalog, MockBodyProvider } from '../src/provider/mock-provider';
import { ProviderError } from '../src/provider/contracts';
import { orderedGroups } from '../src/ui/view-model';

describe('synthetic scenario builders', () => {
  it('keeps surface triangle winding aligned with outward normals and radial winding unchanged', () => {
    for (const irregular of [false, true]) {
      const geometry = buildGeometry('surface-mesh', 7, irregular);
      const checked = assertPositiveWinding(geometry.positions, geometry.normals, geometry.indices);
      expect(checked).toBeGreaterThan(10_000);
    }

    const radial = buildGeometry('radial-profile', 7);
    const radialNormals = new Float32Array(radial.positions.length);
    for (let index = 2; index < radialNormals.length; index += 3) radialNormals[index] = 1;
    expect(assertPositiveWinding(radial.positions, radialNormals, radial.indices)).toBeGreaterThan(8_000);
  });

  it('produce deterministic descriptors and provider responses from a scenario seed', () => {
    for (const scenario of SCENARIOS) {
      const first = buildFixture(scenario);
      const second = buildFixture(scenario);
      expect(first.summary).toEqual(second.summary);
      expect(first.domains).toEqual(second.domains);
      expect([...first.catalogs]).toEqual([...second.catalogs]);
      expect(first.pointReport).toEqual(second.pointReport);
      expect(first.diagnosticStages).toEqual(second.diagnosticStages);
      expect(first.featureCatalog).toEqual(second.featureCatalog);
    }
  });

  it('contains the named shape and scale pressure cases', () => {
    const get = (id: string) => {
      const scenario = SCENARIOS.find((candidate) => candidate.id === id);
      if (!scenario) throw new Error(`Missing scenario ${id}`);
      return { scenario, fixture: buildFixture(scenario) };
    };
    const empty = get('fixture:void').fixture;
    expect(empty.domains).toHaveLength(0);
    expect([...empty.catalogs.values()].flatMap(({ views }) => views)).toHaveLength(0);

    const minimal = get('fixture:minimal').fixture;
    expect(minimal.domains).toHaveLength(1);
    expect([...minimal.catalogs.values()].flatMap(({ views }) => views)).toHaveLength(1);
    expect(minimal.pointReport.groups.reduce((sum, group) => sum + group.fields.length, 0)).toBe(1);

    expect([...get('fixture:normal-surface').fixture.catalogs.values()][0]?.views).toHaveLength(20);
    expect([...get('fixture:view-catalog-extreme').fixture.catalogs.values()][0]?.views.length).toBeGreaterThanOrEqual(50);
    expect(get('fixture:category-heavy').fixture.catalogs.get('domain-surface')?.views[0]?.legend.kind).toBe('categorical');
    expect(get('fixture:point-heavy').fixture.pointReport.groups.reduce((sum, group) => sum + group.fields.length, 0)).toBeGreaterThanOrEqual(30);
    expect(get('fixture:provenance-heavy').fixture.explainDepth).toBeGreaterThanOrEqual(10);
    expect(get('fixture:diagnostics').fixture.diagnosticStages?.length).toBeGreaterThanOrEqual(15);
    expect(get('fixture:multi-domain').fixture.domains.length).toBeGreaterThanOrEqual(3);
  });

  it('uses provider group labels and order without a frontend vocabulary', () => {
    const fixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:view-catalog-extreme')!);
    const catalog = fixture.catalogs.get(fixture.domains[0]!.id)!;
    const groups = orderedGroups(catalog);
    expect(groups.map(({ group }) => group.id)).toEqual([...catalog.groups].sort((a, b) => a.order - b.order).map(({ id }) => id));
    expect(groups.flatMap(({ views }) => views)).toEqual(catalog.views);
  });
});

function assertPositiveWinding(
  positions: Float32Array,
  normals: Float32Array,
  indices: Uint32Array,
): number {
  let checked = 0;
  for (let index = 0; index < indices.length; index += 3) {
    const first = indices[index]!;
    const second = indices[index + 1]!;
    const third = indices[index + 2]!;
    const point = (vertex: number) => [positions[vertex * 3]!, positions[vertex * 3 + 1]!, positions[vertex * 3 + 2]!] as const;
    const normal = (vertex: number) => [normals[vertex * 3]!, normals[vertex * 3 + 1]!, normals[vertex * 3 + 2]!] as const;
    const p0 = point(first);
    const p1 = point(second);
    const p2 = point(third);
    const edge1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    const edge2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
    const geometric = [
      edge1[1]! * edge2[2]! - edge1[2]! * edge2[1]!,
      edge1[2]! * edge2[0]! - edge1[0]! * edge2[2]!,
      edge1[0]! * edge2[1]! - edge1[1]! * edge2[0]!,
    ];
    const length = Math.hypot(...geometric);
    if (length < 1e-8) continue;
    const expected = [0, 1, 2].map((axis) => normal(first)[axis]! + normal(second)[axis]! + normal(third)[axis]!);
    const dot = geometric[0]! * expected[0]! + geometric[1]! * expected[1]! + geometric[2]! * expected[2]!;
    expect(dot).toBeGreaterThan(0);
    checked += 1;
  }
  return checked;
}

describe('MockBodyProvider contract', () => {
  it('returns descriptor-derived counts and optional data across generated scenarios', async () => {
    for (const scenario of SCENARIOS) {
      const fixture = buildFixture(scenario);
      const provider = new MockBodyProvider(fixture);
      if (fixture.metadataFailure) await expect(provider.summary()).rejects.toMatchObject({ failure: fixture.metadataFailure });
      else expect(await provider.summary()).toEqual(fixture.summary);
      expect(await provider.domains()).toEqual(fixture.domains);
      for (const domain of fixture.domains) {
        const catalog = await provider.views(domain.id);
        expect(catalog).toEqual(fixture.catalogs.get(domain.id));
        const geometry = await provider.domainGeometry(domain.id);
        expect(geometry.kind).toBe(domain.renderKind);
        for (const view of catalog.views) {
          expect(await provider.stats(view.id)).toEqual(view.legend.kind === 'continuous' && view.legend.statsAvailable ? expect.objectContaining({ min: expect.any(Number), max: expect.any(Number), mean: expect.any(Number) }) : undefined);
        }
        const view = catalog.views[0];
        if (view && !fixture.viewDelayMs) {
          const tile = await provider.tile(view.id, view.timeSelections?.[0]?.id);
          expect(tile.values).toHaveLength(tile.width * tile.height);
        }
      }
      const report = await provider.inspect({ kind: 'surface-direction', direction: [0, 0, 1] });
      expect(report).toEqual(fixture.pointReport);
      expect(await provider.features()).toEqual(fixture.featureCatalog);
    }
  }, 15_000);

  it('scripts explicit open, metadata, partial, and retry states', async () => {
    const catalog = new MockBodyCatalog();
    await expect(catalog.open('fixture:missing-content')).rejects.toMatchObject({ failure: { code: 'E_CONTENT_MISSING', offendingItem: 'fixture/body-metadata' } });
    await expect(catalog.open('fixture:unsupported-critical')).rejects.toMatchObject({ failure: { code: 'E_CRITICAL_FEATURE_UNSUPPORTED' } });

    const validation = await catalog.open('fixture:validation-failure');
    await expect(validation.summary()).rejects.toMatchObject({ failure: { code: 'E_VALIDATION_DESCRIPTOR', offendingItem: 'view:fixture-invalid' } });

    await expect(catalog.open('fixture:retryable-error')).rejects.toMatchObject({ failure: { retryable: true } });
    expect(await (await catalog.open('fixture:retryable-error')).summary()).toMatchObject({ name: 'Retry fixture' });

    const partial = await catalog.open('fixture:partial-data');
    const view = (await partial.views('domain-surface')).views[0]!;
    expect((await partial.tile(view.id)).missingResources).toEqual(['tile/03', 'metadata/index-02']);
  });

  it('reports provider failures with stable codes', async () => {
    const catalog = new MockBodyCatalog();
    try { await catalog.open('fixture:non-retryable-error'); } catch (error) {
      expect(error).toBeInstanceOf(ProviderError);
      expect((error as ProviderError).failure).toMatchObject({ code: 'E_RESOURCE_UNAVAILABLE', retryable: false });
    }
  });
});
