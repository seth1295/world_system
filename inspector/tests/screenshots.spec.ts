import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import { buildFixture, SCENARIOS } from '../src/fixtures/scenarios';

const output = join('screenshots', 'stress');

test('captures the adaptability evidence matrix', async ({ page }) => {
  test.setTimeout(240_000);
  await mkdir(output, { recursive: true });

  await captureReady(page, 'fixture:normal-surface', { width: 3440, height: 1440 });
  await screenshot(page, '3440-normal-surface.jpg');

  await captureReady(page, 'fixture:view-catalog-extreme', { width: 3440, height: 1440 });
  await page.getByTestId('debug-button').click();
  await expect(page.getByRole('dialog', { name: 'View catalogue' })).toBeVisible();
  await screenshot(page, '3440-view-catalog-extreme-menu.jpg');

  await captureReady(page, 'fixture:point-heavy', { width: 3440, height: 1440 });
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await screenshot(page, '3440-point-heavy-inspection.jpg');

  await captureReady(page, 'fixture:category-heavy', { width: 3440, height: 1440 });
  await screenshot(page, '3440-category-heavy-legend.jpg');

  await captureReady(page, 'fixture:normal-surface', { width: 1440, height: 900 });
  await screenshot(page, '1440-normal-surface.jpg');

  await captureReady(page, 'fixture:radial', { width: 1440, height: 900 });
  await screenshot(page, '1440-radial-profile.jpg');

  await captureReady(page, 'fixture:diagnostics', { width: 1440, height: 900 });
  await page.getByRole('button', { name: /Diagnostics/ }).click();
  await page.locator('[data-stage-id="stage-16"]').click();
  await expect(page.locator('.stage-snapshot')).toBeVisible();
  await screenshot(page, '1440-diagnostics-many-stages.jpg');

  await captureReady(page, 'fixture:normal-surface', { width: 820, height: 1100 });
  await screenshot(page, '820-compact-controls.jpg');

  await captureReady(page, 'fixture:point-heavy', { width: 820, height: 1100 });
  await page.getByRole('button', { name: 'Inspect point' }).click();
  const pointFixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:point-heavy')!);
  await expect(page.locator('#inspection-panel .field-row')).toHaveCount(pointFixture.pointReport.groups.reduce((sum, group) => sum + group.fields.length, 0));
  await screenshot(page, '820-large-point-report.jpg');

  await captureReady(page, 'fixture:view-catalog-extreme', { width: 820, height: 1100 });
  await page.getByTestId('debug-button').click();
  const extreme = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:view-catalog-extreme')!);
  await expect(page.locator('#debug-menu [data-view-id]')).toHaveCount(extreme.catalogs.get(extreme.domains[0]!.id)!.views.length);
  await screenshot(page, '820-debug-menu-overflow.jpg');

  await captureReady(page, 'fixture:multi-domain', { width: 820, height: 1100 });
  await page.locator('#domain-selector').selectOption('domain-profile');
  await expect(page.locator('#status-domain')).toContainText('radial_1d/1');
  await screenshot(page, '820-multi-domain-profile.jpg');

  await page.setViewportSize({ width: 1440, height: 900 });
  await openFixture(page, 'fixture:loading');
  await expect(page.locator('#provider-state')).toContainText('Opening fixture');
  await screenshot(page, '1440-loading-state.jpg');

  await captureReady(page, 'fixture:partial-data', { width: 1440, height: 900 });
  await expect(page.locator('#provider-state')).toContainText('INCOMPLETE DATA');
  await screenshot(page, '1440-partial-data.jpg');

  await openFixture(page, 'fixture:validation-failure');
  await expect(page.locator('.error-code')).toHaveText('E_VALIDATION_DESCRIPTOR');
  await screenshot(page, '1440-validation-failure.jpg');

  await openFixture(page, 'fixture:retryable-error');
  await expect(page.locator('.error-code')).toHaveText('E_RESOURCE_TEMPORARY');
  await screenshot(page, '1440-retryable-error.jpg');

  await captureReady(page, 'fixture:void', { width: 1440, height: 900 });
  await screenshot(page, '1440-void.jpg');

  await captureReady(page, 'fixture:minimal', { width: 1440, height: 900 });
  await screenshot(page, '1440-minimal.jpg');
});

async function captureReady(page: Page, fixtureId: string, viewport: { width: number; height: number }): Promise<void> {
  await page.setViewportSize(viewport);
  await openFixture(page, fixtureId);
  await expect.poll(async () => page.locator('#provider-state').getAttribute('data-state'), { timeout: 20_000 }).toBe('ready');
  await page.waitForTimeout(180);
}

async function openFixture(page: Page, fixtureId: string): Promise<void> {
  if (page.url() === 'about:blank') {
    await page.goto(`/?fixture=${encodeURIComponent(fixtureId)}`);
    return;
  }
  if (new URL(page.url()).searchParams.get('fixture') !== fixtureId) await page.locator('#fixture-selector').selectOption(fixtureId);
}

async function screenshot(page: Page, filename: string): Promise<void> {
  await page.screenshot({ path: join(output, filename), type: 'jpeg', quality: 76, fullPage: false, animations: 'disabled' });
}
