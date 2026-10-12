export const MAX_PROFILE_PLOT_POINTS = 200;

export interface RadialProfilePlot {
  sourceSampleCount: number;
  minimum: number;
  maximum: number;
  sampleIndices: readonly number[];
  points: string;
}

/** Reduce provider profile samples for a fixed-size Inspector plot, preserving range peaks. */
export function buildRadialProfilePlot(values: readonly number[]): RadialProfilePlot | null {
  if (values.length < 2) return null;

  let minimum = Number.POSITIVE_INFINITY;
  let maximum = Number.NEGATIVE_INFINITY;
  let firstFiniteIndex = -1;
  let lastFiniteIndex = -1;
  let finiteCount = 0;
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index]!;
    if (!Number.isFinite(value)) continue;
    if (firstFiniteIndex < 0) firstFiniteIndex = index;
    lastFiniteIndex = index;
    finiteCount += 1;
    if (value < minimum) {
      minimum = value;
    }
    if (value > maximum) {
      maximum = value;
    }
  }
  if (finiteCount < 2) return null;

  const sampleIndices = values.length <= MAX_PROFILE_PLOT_POINTS
    ? finiteIndices(values)
    : reduceProfileIndices(values, firstFiniteIndex, lastFiniteIndex);
  const magnitude = Math.max(Math.abs(minimum), Math.abs(maximum)) || 1;
  const scaledMinimum = minimum / magnitude;
  const scaledRange = maximum / magnitude - scaledMinimum;
  const sourceSpan = values.length - 1;
  const points = sampleIndices.map((index) => {
    const value = values[index]!;
    const x = (index / sourceSpan) * 100;
    const normalized = scaledRange === 0 ? 0 : (value / magnitude - scaledMinimum) / scaledRange;
    const y = 38 - normalized * 32;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  }).join(' ');

  return {
    sourceSampleCount: values.length,
    minimum,
    maximum,
    sampleIndices,
    points,
  };
}

function finiteIndices(values: readonly number[]): number[] {
  const indices: number[] = [];
  for (let index = 0; index < values.length; index += 1) if (Number.isFinite(values[index])) indices.push(index);
  return indices;
}

function reduceProfileIndices(values: readonly number[], first: number, last: number): number[] {
  const bucketCount = Math.floor((MAX_PROFILE_PLOT_POINTS - 2) / 2);
  const minimums = Array.from({ length: bucketCount }, () => ({ index: -1, value: Number.POSITIVE_INFINITY }));
  const maximums = Array.from({ length: bucketCount }, () => ({ index: -1, value: Number.NEGATIVE_INFINITY }));
  const sourceRange = last - first;

  for (let index = first + 1; index < last; index += 1) {
    const value = values[index]!;
    if (!Number.isFinite(value)) continue;
    const bucket = Math.min(bucketCount - 1, Math.floor(((index - first) / sourceRange) * bucketCount));
    if (value < minimums[bucket]!.value) minimums[bucket] = { index, value };
    if (value > maximums[bucket]!.value) maximums[bucket] = { index, value };
  }

  const selected = new Set<number>([first, last]);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const minimum = minimums[bucket]!;
    const maximum = maximums[bucket]!;
    if (minimum.index >= 0) selected.add(minimum.index);
    if (maximum.index >= 0) selected.add(maximum.index);
  }
  return [...selected].sort((left, right) => left - right);
}
