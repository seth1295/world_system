import { describe, expect, it } from 'vitest';
import { buildGeometry } from '../src/fixtures/scenarios';
import { buildRadialProfilePlot, MAX_PROFILE_PLOT_POINTS } from '../src/ui/profile-plot';

describe('radial profile presentation', () => {
  it('keeps the existing radial fixture at full fidelity', () => {
    const values = buildGeometry('radial-profile', 4).profile!;
    const plot = buildRadialProfilePlot(values);
    expect(plot).not.toBeNull();
    expect(plot!.sourceSampleCount).toBe(values.length);
    expect(plot!.sampleIndices).toEqual(Array.from({ length: values.length }, (_, index) => index));
    expect(plot!.sampleIndices).toHaveLength(values.length);
  });

  it('bounds a very large profile while retaining endpoints and global extrema', () => {
    const count = 250_000;
    const minimumIndex = 81_234;
    const maximumIndex = 221_111;
    const values = Array.from({ length: count }, (_, index) => Math.sin(index * 0.017) * 3);
    values[0] = -4;
    values[count - 1] = 5;
    values[minimumIndex] = -1_000;
    values[maximumIndex] = 2_000;

    const plot = buildRadialProfilePlot(values);
    expect(plot).not.toBeNull();
    expect(plot!.sourceSampleCount).toBe(count);
    expect(plot!.minimum).toBe(-1_000);
    expect(plot!.maximum).toBe(2_000);
    expect(plot!.sampleIndices.length).toBeLessThanOrEqual(MAX_PROFILE_PLOT_POINTS);
    expect(plot!.sampleIndices[0]).toBe(0);
    expect(plot!.sampleIndices.at(-1)).toBe(count - 1);
    expect(plot!.sampleIndices).toContain(minimumIndex);
    expect(plot!.sampleIndices).toContain(maximumIndex);
    expect(plot!.points.length).toBeLessThan(MAX_PROFILE_PLOT_POINTS * 16);
    expect(plot!.points).not.toMatch(/NaN|Infinity/);
  });

  it('keeps constant and negative-valued profiles finite', () => {
    const constant = buildRadialProfilePlot([4, 4, 4]);
    expect(constant).toMatchObject({ minimum: 4, maximum: 4, points: '0.00,38.00 50.00,38.00 100.00,38.00' });

    const negative = buildRadialProfilePlot([-8, -2, -5]);
    expect(negative).toMatchObject({ minimum: -8, maximum: -2, sampleIndices: [0, 1, 2] });
    expect(negative!.points).not.toMatch(/NaN|Infinity/);
  });

  it('handles extreme finite bounds without overflow and skips non-finite samples', () => {
    const extremes = buildRadialProfilePlot([-1e308, 0, 1e308]);
    expect(extremes).toMatchObject({ minimum: -1e308, maximum: 1e308 });
    expect(extremes!.points).not.toMatch(/NaN|Infinity/);

    const mixed = buildRadialProfilePlot([Number.NaN, -4, Number.POSITIVE_INFINITY, 4]);
    expect(mixed).toMatchObject({ sourceSampleCount: 4, minimum: -4, maximum: 4, sampleIndices: [1, 3] });
    expect(mixed!.points).not.toMatch(/NaN|Infinity/);
  });

  it('retains the existing empty response for fewer than two samples', () => {
    expect(buildRadialProfilePlot([])).toBeNull();
    expect(buildRadialProfilePlot([3])).toBeNull();
  });
});
