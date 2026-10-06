import { mkdir } from 'node:fs/promises';
import { test, expect } from '@playwright/test';

test('opens all mock bodies, adapts the Debug menu, changes views, and inspects a point', async ({ page }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await expect(page.locator('.brand-lockup')).toBeVisible();
  await expect(page.getByTestId('viewport-canvas')).toBeVisible();
  await expect(page.locator('#body-name')).toHaveText('Veyra');
  await page.waitForTimeout(300);
  await mkdir('screenshots', { recursive: true });
  await page.screenshot({ path: 'screenshots/veyra-default.png', fullPage: true });
  const surfaceLegend = await page.locator('#legend .legend-scale').getAttribute('style');
  const featuresButton = page.locator('#features-button');
  await expect(featuresButton).toBeVisible();
  await featuresButton.click();
  await expect(featuresButton).toHaveAttribute('aria-pressed', 'true');
  await featuresButton.click();
  await expect(featuresButton).toHaveAttribute('aria-pressed', 'false');

  await page.getByTestId('debug-button').click();
  await expect(page.locator('#debug-menu')).toContainText('Topography');
  await expect(page.locator('#debug-menu')).toContainText('Tectonics');
  await page.locator('[data-view-id="height"]').click();
  await expect(page.locator('#view-name')).toHaveText('Height');
  const heightLegend = await page.locator('#legend .legend-scale').getAttribute('style');
  expect(heightLegend).not.toBe(surfaceLegend);
  await page.waitForTimeout(350);
  await page.screenshot({ path: 'screenshots/veyra-topography.png', fullPage: true });

  await page.getByTestId('body-selector').selectOption('obj:6b18e441');
  await expect(page.locator('#body-name')).toHaveText('Auren');
  await page.getByTestId('debug-button').click();
  await expect(page.locator('#debug-menu')).toContainText('Stellar structure');
  await expect(page.locator('#debug-menu')).not.toContainText('Topography');
  await page.getByTestId('debug-button').click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: 'screenshots/auren-radial.png', fullPage: true });

  await page.getByTestId('body-selector').selectOption('obj:c8a0472d');
  await expect(page.locator('#body-name')).toHaveText('Irregular Rock');
  await expect(page.locator('#view-name')).toHaveText('Shape');
  await page.getByTestId('debug-button').click();
  await expect(page.locator('#debug-menu')).toContainText('Surface material');
  await expect(page.locator('#debug-menu')).not.toContainText('Climate');
  await page.keyboard.press('Escape');
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.waitForTimeout(120);
  await page.screenshot({ path: 'screenshots/irregular-rock.png', fullPage: true });

  await page.getByTestId('body-selector').selectOption('obj:9f3c2a7d');
  await expect(page.locator('#body-name')).toHaveText('Veyra');
  const canvas = page.getByTestId('viewport-canvas');
  const bounds = await canvas.boundingBox();
  if (!bounds) throw new Error('The viewport canvas has no layout bounds');
  await canvas.click({ position: { x: bounds.width * 0.47, y: bounds.height * 0.51 } });
  await expect(page.getByTestId('selection-card')).toBeVisible({ timeout: 5_000 });
  await expect(page.locator('#selection-fields')).toContainText('Height');
  await page.screenshot({ path: 'screenshots/veyra-point-inspection.png', fullPage: true });
  expect(pageErrors).toEqual([]);
});
