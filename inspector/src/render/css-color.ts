/** Accept only values the browser parses as a single CSS color. */
export function safeCssColor(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const color = value.trim();
  if (!color || color.length > 128 || /[\u0000-\u001f\u007f]/.test(color)) return null;
  if (/\b(?:var|env|attr)\s*\(/i.test(color) || /^(?:inherit|initial|unset|revert|revert-layer)$/i.test(color)) return null;
  if (typeof CSS === 'undefined' || typeof CSS.supports !== 'function' || !CSS.supports('color', color)) return null;
  return color;
}
