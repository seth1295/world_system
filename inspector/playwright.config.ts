import { defineConfig, devices, type PlaywrightTestConfig } from '@playwright/test';

export function buildPlaywrightConfig(environment: NodeJS.ProcessEnv = process.env): PlaywrightTestConfig {
  const executablePath = environment.CHROMIUM_PATH?.trim();
  return defineConfig({
    testDir: './tests',
    testMatch: '**/*.spec.ts',
    fullyParallel: false,
    reporter: 'list',
    use: {
      ...devices['Desktop Chrome'],
      baseURL: 'http://127.0.0.1:4173',
      headless: true,
      launchOptions: {
        ...(executablePath ? { executablePath } : {}),
        args: ['--no-sandbox', '--enable-webgl', '--ignore-gpu-blocklist', '--use-angle=swiftshader', '--disable-dev-shm-usage'],
      },
    },
    webServer: {
      command: 'npm run dev -- --host 127.0.0.1 --port 4173 --strictPort',
      url: 'http://127.0.0.1:4173',
      reuseExistingServer: !environment.CI,
      timeout: 30_000,
    },
  });
}

export default buildPlaywrightConfig();
