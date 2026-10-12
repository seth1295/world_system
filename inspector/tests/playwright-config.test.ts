import { describe, expect, it } from 'vitest';
import { buildPlaywrightConfig } from '../playwright.config';

describe('Playwright Chromium executable selection', () => {
  it('omits executablePath when CHROMIUM_PATH is unset or blank', () => {
    for (const environment of [{}, { CHROMIUM_PATH: '  ' }]) {
      const launchOptions = buildPlaywrightConfig(environment).use?.launchOptions;
      expect(launchOptions).toBeDefined();
      expect(launchOptions).not.toHaveProperty('executablePath');
    }
  });

  it('uses CHROMIUM_PATH only when explicitly supplied', () => {
    const launchOptions = buildPlaywrightConfig({ CHROMIUM_PATH: '  /custom/chromium  ' }).use?.launchOptions;
    expect(launchOptions).toHaveProperty('executablePath', '/custom/chromium');
  });
});
