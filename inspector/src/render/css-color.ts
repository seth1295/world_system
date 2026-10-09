export interface NormalizedColor {
  css: string;
  hex: string;
  rgba: readonly [number, number, number, number];
}

export interface NormalizedPaletteStop {
  at: number;
  color: NormalizedColor;
}

export const DEFAULT_CATEGORICAL_COLOR: NormalizedColor = {
  css: 'rgb(105, 113, 120)',
  hex: '#697178',
  rgba: [105, 113, 120, 255],
};

const DEFAULT_CONTINUOUS_PALETTE: readonly NormalizedPaletteStop[] = [
  { at: 0, color: { css: 'rgb(85, 96, 106)', hex: '#55606a', rgba: [85, 96, 106, 255] } },
  { at: 1, color: { css: 'rgb(238, 240, 236)', hex: '#eef0ec', rgba: [238, 240, 236, 255] } },
];

let colorContext: CanvasRenderingContext2D | null | undefined;
const colorCache = new Map<string, NormalizedColor | null>();

/** Parse one standalone opaque CSS color into CSS and sRGB values used by both render paths. */
export function normalizeCssColor(value: unknown): NormalizedColor | null {
  return normalizeCssColorValue(value, false);
}

/** Parse one standalone CSS color while preserving its alpha for feature overlays. */
export function normalizeCssColorWithAlpha(value: unknown): NormalizedColor | null {
  return normalizeCssColorValue(value, true);
}

function normalizeCssColorValue(value: unknown, allowAlpha: boolean): NormalizedColor | null {
  if (typeof value !== 'string') return null;
  if (value.length > 128 || /[\u0000-\u001f\u007f]/.test(value)) return null;
  const source = value.trim();
  if (!source) return null;
  if (/\b(?:var|env|attr)\s*\(/i.test(source) || /^(?:inherit|initial|unset|revert|revert-layer|currentcolor)$/i.test(source)) return null;
  const cacheKey = `${allowAlpha ? 'alpha' : 'opaque'}:${source}`;
  const cached = colorCache.get(cacheKey);
  if (cached !== undefined) return cached;
  if (typeof CSS === 'undefined' || typeof CSS.supports !== 'function' || !CSS.supports('color', source)) {
    colorCache.set(cacheKey, null);
    return null;
  }
  const context = getColorContext();
  if (!context) {
    colorCache.set(cacheKey, null);
    return null;
  }

  let parsedStyle: string | null = null;
  for (const sentinel of ['#010203', '#040506']) {
    context.fillStyle = sentinel;
    const priorStyle = context.fillStyle;
    context.fillStyle = source;
    const nextStyle = context.fillStyle;
    if (nextStyle !== priorStyle) {
      parsedStyle = nextStyle;
      break;
    }
  }
  if (!parsedStyle) {
    colorCache.set(cacheKey, null);
    return null;
  }

  context.clearRect(0, 0, 1, 1);
  context.fillStyle = parsedStyle;
  context.fillRect(0, 0, 1, 1);
  const pixel = context.getImageData(0, 0, 1, 1).data;
  if (!allowAlpha && pixel[3] !== 255) {
    colorCache.set(cacheKey, null);
    return null;
  }
  const rgba = (allowAlpha ? parseSerializedCanvasColor(parsedStyle) : null)
    ?? [pixel[0]!, pixel[1]!, pixel[2]!, pixel[3]!] as const;
  const normalized = colorFromRgba(rgba);
  colorCache.set(cacheKey, normalized);
  return normalized;
}

function parseSerializedCanvasColor(value: string): readonly [number, number, number, number] | null {
  const hex = value.match(/^#([\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/i)?.[1];
  if (hex) {
    const channels = hex.length <= 4
      ? [...hex].map((channel) => Number.parseInt(channel + channel, 16))
      : hex.match(/.{2}/g)!.map((channel) => Number.parseInt(channel, 16));
    return [channels[0]!, channels[1]!, channels[2]!, channels[3] ?? 255];
  }
  const match = value.match(/^rgba?\(\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+))(%?)\s*,\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+))(%?)\s*,\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+))(%?)(?:\s*,\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+))(%?))?\s*\)$/i);
  if (!match) return null;
  const channel = (value: string, percent: string): number => Math.round(Math.max(0, Math.min(255, Number(value) * (percent ? 2.55 : 1))));
  const alpha = match[7] === undefined
    ? 255
    : Math.round(Math.max(0, Math.min(255, Number(match[7]) * (match[8] ? 2.55 : 255))));
  return [channel(match[1]!, match[2]!), channel(match[3]!, match[4]!), channel(match[5]!, match[6]!), alpha];
}

/** Keep the legend and raster renderer on the same validated, ordered stop set. */
export function normalizePaletteStops(stops: readonly { at: number; color: string }[]): readonly NormalizedPaletteStop[] {
  const normalized = stops.flatMap(({ at, color }) => {
    if (typeof at !== 'number' || !Number.isFinite(at) || at < 0 || at > 1) return [];
    const normalizedColor = normalizeCssColor(color);
    return normalizedColor ? [{ at, color: normalizedColor }] : [];
  }).sort((left, right) => left.at - right.at);
  return normalized.length ? normalized : DEFAULT_CONTINUOUS_PALETTE;
}

/** Interpolate the same normalized sRGB bytes that the viewport writes into its texture. */
export function sampleNormalizedPalette(stops: readonly NormalizedPaletteStop[], value: number): NormalizedColor {
  const low = [...stops].reverse().find((stop) => stop.at <= value) ?? stops[0] ?? DEFAULT_CONTINUOUS_PALETTE[0]!;
  const high = stops.find((stop) => stop.at >= value) ?? stops.at(-1) ?? low;
  const span = Math.max(0.0001, high.at - low.at);
  const weight = Math.max(0, Math.min(1, (value - low.at) / span));
  const rgba = [0, 1, 2, 3].map((channel) => Math.round(low.color.rgba[channel]! + (high.color.rgba[channel]! - low.color.rgba[channel]!) * weight)) as [number, number, number, number];
  return colorFromRgba(rgba);
}

function getColorContext(): CanvasRenderingContext2D | null {
  if (colorContext !== undefined) return colorContext;
  if (typeof document === 'undefined') {
    colorContext = null;
    return colorContext;
  }
  const canvas = document.createElement('canvas');
  canvas.width = 1;
  canvas.height = 1;
  colorContext = canvas.getContext('2d', { willReadFrequently: true });
  return colorContext;
}

function colorFromRgba(rgba: readonly [number, number, number, number]): NormalizedColor {
  const hex = `#${rgba.slice(0, 3).map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
  const alpha = Number((rgba[3] / 255).toFixed(6));
  return {
    css: rgba[3] === 255 ? `rgb(${rgba[0]}, ${rgba[1]}, ${rgba[2]})` : `rgba(${rgba[0]}, ${rgba[1]}, ${rgba[2]}, ${alpha})`,
    hex,
    rgba,
  };
}
