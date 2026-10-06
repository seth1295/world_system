import { describe, expect, it } from 'vitest';
import { MockBodyCatalog } from '../src/mock-provider';
import { groupViews } from '../src/ui/view-model';

describe('mock body descriptor boundary', () => {
  it('declares different view groups from each selected mock body', async () => {
    const catalog = new MockBodyCatalog();
    const bodies = await catalog.bodies();
    expect(bodies).toHaveLength(3);

    const veyra = await catalog.open(bodies[0]!.objectId);
    const auren = await catalog.open(bodies[1]!.objectId);
    const rock = await catalog.open(bodies[2]!.objectId);

    expect([...groupViews(await veyra.views()).keys()]).toEqual(['Spatial', 'Topography', 'Tectonics', 'Ocean', 'Climate']);
    expect([...groupViews(await auren.views()).keys()]).toEqual(['Stellar structure']);
    expect([...groupViews(await rock.views()).keys()]).toEqual(['Spatial', 'Solid surface', 'Surface material', 'Thermal state']);
  });

  it('returns presentation geometry and representative point values through the provider contract', async () => {
    const catalog = new MockBodyCatalog();
    const bodies = await catalog.bodies();
    const auren = await catalog.open(bodies[1]!.objectId);
    const geometry = await auren.domainGeometry();
    const point = await auren.inspect({ kind: 'radial-distance', normalizedRadius: 0.64 });

    expect(geometry.kind).toBe('radial-profile');
    expect(point.fields.map((field) => field.label)).toContain('Hydrogen fraction');
    expect(point.source).toContain('Mock provider');
    expect(await auren.radialProfile('star-density')).toHaveLength(64);
  });
});
