import { test, expect, type Page } from '@playwright/test';
import { buildFixture, SCENARIOS } from '../src/fixtures/scenarios';
import { MAX_PROFILE_PLOT_POINTS } from '../src/ui/profile-plot';
import { orderedGroups } from '../src/ui/view-model';

const viewSizes = [
  { width: 3440, height: 1440 },
  { width: 1440, height: 900 },
  { width: 820, height: 1100 },
];

test('alternate catalog without a fixture query opens its first option and canonicalizes the URL', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/tests/catalog-harness.html');
  await waitForReady(page);
  await expect(page.locator('#fixture-selector')).toHaveValue('body:alpha');
  await expect(page.locator('#fixture-selector option')).toHaveCount(2);
  expect(await page.evaluate(() => window.catalogHarness.opened)).toEqual(['body:alpha']);
  expect(new URL(page.url()).searchParams.get('fixture')).toBe('body:alpha');
});

test('alternate catalog with an invalid fixture query falls back to its first option', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/tests/catalog-harness.html?fixture=stale%3Aentry');
  await waitForReady(page);
  await expect(page.locator('#fixture-selector')).toHaveValue('body:alpha');
  expect(await page.evaluate(() => window.catalogHarness.opened)).toEqual(['body:alpha']);
  expect(new URL(page.url()).searchParams.get('fixture')).toBe('body:alpha');
});

test('alternate catalog preserves a valid query for a non-first fixture', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/tests/catalog-harness.html?fixture=body%3Abeta');
  await waitForReady(page);
  await expect(page.locator('#fixture-selector')).toHaveValue('body:beta');
  expect(await page.evaluate(() => window.catalogHarness.opened)).toEqual(['body:beta']);
  expect(new URL(page.url()).searchParams.get('fixture')).toBe('body:beta');
});

test('empty catalog presents an unavailable state without opening or advertising a fixture', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/tests/catalog-harness.html?empty=1&fixture=stale%3Aentry');
  await expect(page.locator('#provider-state')).toHaveAttribute('data-state', 'ready');
  await expect(page.locator('#body-name')).toHaveText('No fixtures available');
  await expect(page.locator('#view-name')).toHaveText('No fixtures available');
  await expect(page.locator('#fixture-selector')).toBeDisabled();
  await expect(page.locator('#fixture-selector option')).toHaveCount(0);
  expect(await page.evaluate(() => window.catalogHarness.opened)).toEqual([]);
  expect(new URL(page.url()).searchParams.get('fixture')).toBe('stale:entry');
});

test('void fixture is stable, empty, and produces no browser errors', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Avoid');
  await waitForReady(page);
  await expect(page.locator('#body-name')).toHaveText('Empty fixture');
  await expect(page.getByTestId('debug-button')).toBeDisabled();
  await expect(page.getByTestId('debug-button')).toHaveText('Debug · none');
  await expect(page.locator('#debug-menu .catalog-group')).toHaveCount(0);
  await expect(page.locator('#view-name')).toHaveText('No view declared');
  expect(errors).toEqual([]);
});

test('canvas picking opens a point report through the provider', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Anormal-surface');
  await waitForReady(page);
  const canvas = page.getByTestId('viewport-canvas');
  const bounds = await canvas.boundingBox();
  expect(bounds).not.toBeNull();
  await canvas.click({ position: { x: bounds!.width * 0.5, y: bounds!.height * 0.5 } });
  await expect(page.locator('#inspection-panel')).toBeVisible();
  const point = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:normal-surface')!).pointReport;
  await expect(page.locator('#inspection-panel .field-row')).toHaveCount(point.groups.reduce((sum, group) => sum + group.fields.length, 0));
});

test('descriptor and provider response counts match generated fixture data', async ({ page }) => {
  test.setTimeout(180_000);
  const successScenarios = SCENARIOS.filter(({ openFailure, failFirstOpen, metadataFailure }) => !openFailure && !failFirstOpen && !metadataFailure);
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Avoid');
  await waitForReady(page);
  for (const scenario of successScenarios) {
    const fixture = buildFixture(scenario);
    await switchFixture(page, scenario.id);
    await expect(page.locator('.synthetic-tag')).toHaveText('SYNTHETIC FIXTURE');

    const defaultDomain = fixture.domains[0];
    const catalog = defaultDomain ? fixture.catalogs.get(defaultDomain.id) : undefined;
    const views = catalog?.views ?? [];
    const groups = catalog ? orderedGroups(catalog) : [];
    const active = views[0];
    const categoryCount = active?.legend.kind === 'categorical' ? active.legend.categories.length : 0;
    expect(await page.locator('#legend-panel .category-row').count()).toBe(categoryCount);

    if (views.length > 1) {
      await page.getByTestId('debug-button').click();
      expect(await page.locator('#debug-menu .catalog-group').count()).toBe(groups.length);
      expect(await page.locator('#debug-menu [data-view-id]').count()).toBe(views.length);
      await page.keyboard.press('Escape');
    } else {
      await expect(page.getByTestId('debug-button')).toBeDisabled();
    }

    if (fixture.pointReport.groups.length > 0) {
      await page.getByRole('button', { name: 'Inspect point' }).click();
      await expect(page.locator('#inspection-panel')).toBeVisible();
      const expectedFields = fixture.pointReport.groups.reduce((sum, group) => sum + group.fields.length, 0);
      expect(await page.locator('#inspection-panel .field-row').count()).toBe(expectedFields);
      await page.locator('.close-inspection').click();
    }

    const stages = fixture.diagnosticStages ?? [];
    if (stages.length > 0) {
      await page.getByRole('button', { name: /Diagnostics/ }).click();
      expect(await page.locator('#diagnostics-menu [data-stage-id]').count()).toBe(stages.length);
      await page.keyboard.press('Escape');
    } else {
      await expect(page.locator('#diagnostics-button')).toBeHidden();
    }

    const tables = fixture.featureCatalog.tables;
    const hasGeometry = tables.some(({ geometry }) => geometry !== undefined);
    if (hasGeometry) {
      await page.getByRole('button', { name: /Features/ }).click();
      expect(await page.locator('#features-menu .feature-option').count()).toBe(tables.length);
      const firstGeometry = page.locator('#features-menu input[data-feature-id]:not([disabled])').first();
      await firstGeometry.check();
      await expect(firstGeometry).toBeChecked();
      await page.keyboard.press('Escape');
    } else {
      await expect(page.locator('#features-button')).toBeHidden();
    }
  }
});

test('extreme view catalogue is bounded, searchable, and traversable with the keyboard at three sizes', async ({ page }) => {
  const fixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:view-catalog-extreme')!);
  const catalog = fixture.catalogs.get(fixture.domains[0]!.id)!;
  await page.setViewportSize(viewSizes[0]!);
  await page.goto('/?fixture=fixture%3Aview-catalog-extreme');
  await waitForReady(page);
  for (const size of viewSizes) {
    await page.setViewportSize(size);
    await page.keyboard.press('Escape');
    await page.getByTestId('debug-button').click();
    const menu = page.getByRole('dialog', { name: 'View catalogue' });
    await expect(menu).toBeVisible();
    await expect(menu.locator('[data-view-id]')).toHaveCount(catalog.views.length);
    const bounds = await menu.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(size.width + 1);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(size.height + 1);

    const search = page.getByRole('searchbox', { name: 'Search views' });
    const last = catalog.views.at(-1)!;
    await search.fill(last.label);
    await expect(menu.locator('[data-view-id]:visible')).toHaveCount(1);
    await expect(menu.locator(`[data-view-id="${last.id}"]`)).toBeVisible();
    await search.fill('no descriptor can match this phrase');
    await expect(page.locator('#debug-empty')).toBeVisible();
    await search.fill('');
    await expect(menu.locator('[data-view-id]:visible')).toHaveCount(catalog.views.length);

    await search.focus();
    await page.keyboard.press('ArrowDown');
    for (const view of catalog.views) {
      await expect(page.locator(':focus')).toHaveAttribute('data-view-id', view.id);
      await page.keyboard.press('ArrowDown');
    }
    await page.keyboard.press('Enter');
    await expect(menu).toBeHidden();
    await expect(page.getByTestId('debug-button')).toBeFocused();
    await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', last.id);
  }
});

test('temporal choices remain separate from view selection and preserve descriptor labels', async ({ page }) => {
  const fixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:temporal-heavy')!);
  const views = fixture.catalogs.get(fixture.domains[0]!.id)!.views;
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Atemporal-heavy');
  await waitForReady(page);
  for (const view of views) {
    if (view.id !== views[0]!.id) {
      await page.getByTestId('debug-button').click();
      await page.locator(`[data-view-id="${view.id}"]`).click();
      await waitForReady(page);
    }
    await expect(page.locator('#temporal-control')).toBeVisible();
    expect(await page.locator('#time-selector option').count()).toBe(view.timeSelections?.length);
    const finalSelection = view.timeSelections?.at(-1);
    if (finalSelection) {
      await page.locator('#time-selector').selectOption(finalSelection.id);
      await waitForReady(page);
      await expect(page.locator(`#time-selector option[value="${finalSelection.id}"]`)).toHaveAttribute('title', finalSelection.label);
    }
  }
});

test('dense legends keep all provider categories reachable and show optional data explicitly', async ({ page }) => {
  const fixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:category-heavy')!);
  const catalog = fixture.catalogs.get(fixture.domains[0]!.id)!;
  const categories = catalog.views[0]!.legend;
  if (categories.kind !== 'categorical') throw new Error('Category fixture did not declare categorical data');
  await page.setViewportSize(viewSizes[0]!);
  await page.goto('/?fixture=fixture%3Acategory-heavy');
  await waitForReady(page);
  const list = page.locator('.category-list');
  await expect(list.locator('.category-row')).toHaveCount(categories.categories.length);
  const last = list.locator('.category-row').last();
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeVisible();
  await expect(last).toHaveAttribute('title', categories.categories.at(-1)!.label);
  const bounds = await page.locator('#legend-panel').boundingBox();
  expect(bounds!.height).toBeLessThan(viewSizes[0]!.height * 0.45);
  expect(await list.evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);

  const hugeRange = catalog.views[1]!;
  await page.getByTestId('debug-button').click();
  await page.locator(`[data-view-id="${hugeRange.id}"]`).click();
  await waitForReady(page);
  await expect(page.locator('#legend-panel')).toContainText('No unit provided');
  await expect(page.locator('#legend-panel .range-value')).toContainText('e');
  await expect(page.locator('#legend-panel .no-stats')).toHaveText('No statistics available');
  const noStatsView = catalog.views[2]!;
  await page.getByTestId('debug-button').click();
  await page.locator(`[data-view-id="${noStatsView.id}"]`).click();
  await waitForReady(page);
  await expect(page.locator('#legend-panel .no-stats')).toBeVisible();
});

test('large point and explanation reports stay bounded, searchable, and copyable', async ({ page }) => {
  const pointFixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:point-heavy')!);
  const expectedFields = pointFixture.pointReport.groups.reduce((sum, group) => sum + group.fields.length, 0);
  await page.setViewportSize(viewSizes[2]!);
  await page.goto('/?fixture=fixture%3Apoint-heavy');
  await waitForReady(page);
  await page.getByRole('button', { name: 'Inspect point' }).click();
  const panel = page.locator('#inspection-panel');
  await expect(panel).toBeVisible();
  await expect(panel.locator('.field-row')).toHaveCount(expectedFields);
  const bounds = await panel.boundingBox();
  expect(bounds!.height).toBeLessThan(viewSizes[2]!.height);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.getByRole('searchbox', { name: 'Filter point fields' }).fill('Field 46');
  await expect(panel.locator('.field-row:visible')).toHaveCount(1);
  await expect(panel.locator('.field-row:visible .field-name')).toHaveAttribute('title', /Field 46/);
  await page.getByRole('searchbox', { name: 'Filter point fields' }).fill('');
  await page.getByRole('button', { name: 'Copy position' }).click();
  await expect(page.locator('#copy-status')).toHaveText(/Copied|Clipboard unavailable/);

  const explainFixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:provenance-heavy')!);
  await page.goto('/?fixture=fixture%3Aprovenance-heavy');
  await waitForReady(page);
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect(page.locator('.explain-step')).toHaveCount(explainFixture.explainDepth);
  await page.getByText('Provider explanation').click();
  for (let level = 1; level <= explainFixture.explainDepth; level += 1) {
    const step = page.locator('.explain-step').nth(level - 1);
    await step.locator(':scope > summary').click();
  }
  await expect(page.locator('.reference-list')).toHaveCount(explainFixture.explainDepth);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test('domain switching replaces descriptors, geometry, and the selected report', async ({ page }) => {
  const fixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:multi-domain')!);
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Amulti-domain');
  await waitForReady(page);
  const before = await page.locator('#legend-panel').getAttribute('data-view-id');
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await page.locator('#domain-selector').selectOption(fixture.domains[1]!.id);
  await waitForReady(page);
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await expect(page.locator('#status-domain')).toContainText(fixture.domains[1]!.topology);
  expect(await page.locator('#legend-panel').getAttribute('data-view-id')).not.toBe(before);
  await page.getByTestId('debug-button').click();
  const nextCatalog = fixture.catalogs.get(fixture.domains[1]!.id)!;
  expect(await page.locator('#debug-menu [data-view-id]').count()).toBe(nextCatalog.views.length);
});

test('loading, partial response, and each explicit provider failure are visible', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Aloading');
  await expect(page.locator('#provider-state')).toContainText('Opening fixture');
  await expect.poll(async () => await page.locator('#provider-state').getAttribute('data-state')).toBe('metadata');
  await expect(page.locator('#provider-state')).toContainText('Loading provider metadata');
  await waitForReady(page);

  await page.locator('#fixture-selector').selectOption('fixture:view-loading');
  await expect(page.locator('#provider-state')).toContainText('Loading view data');
  await waitForReady(page);

  await switchFixture(page, 'fixture:partial-data');
  await waitForReady(page);
  await expect(page.locator('#provider-state')).toContainText('INCOMPLETE DATA');
  await expect(page.locator('#legend-panel .incomplete-note')).toContainText('tile/03');

  const cases = [
    ['missing-content', 'E_CONTENT_MISSING', 'Required content missing', 'Retry'],
    ['validation-failure', 'E_VALIDATION_DESCRIPTOR', 'Provider validation failure', 'Retry'],
    ['unsupported-critical', 'E_CRITICAL_FEATURE_UNSUPPORTED', 'Unsupported required feature', 'Retry'],
    ['retryable-error', 'E_RESOURCE_TEMPORARY', 'Temporary load error', 'Recover'],
    ['non-retryable-error', 'E_RESOURCE_UNAVAILABLE', 'Provider load error', 'NoRetry'],
  ] as const;
  for (const [fixtureId, code, title, action] of cases) {
    await switchFixture(page, `fixture:${fixtureId}`);
    const error = page.getByRole('alert');
    await expect(error).toBeVisible();
    await expect(error.locator('.error-code')).toHaveText(code);
    await expect(error.locator('h2')).toHaveText(title);
    if (action === 'Recover') {
      await page.getByRole('button', { name: 'Retry' }).click();
      await waitForReady(page);
      await expect(page.locator('#provider-error')).toBeHidden();
    } else if (action === 'NoRetry') {
      await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
    } else {
      await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
    }
  }
});

test('diagnostic stage selection changes the displayed provider snapshot', async ({ page }) => {
  const fixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:diagnostics')!);
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Adiagnostics');
  await waitForReady(page);
  await page.getByRole('button', { name: /Diagnostics/ }).click();
  const stages = page.locator('#diagnostics-menu [data-stage-id]:visible');
  await expect(stages).toHaveCount(fixture.diagnosticStages!.length);
  await page.locator('#diagnostic-search').fill(fixture.diagnosticStages!.at(-1)!.label);
  await expect(stages).toHaveCount(1);
  await stages.first().click();
  await expect(page.locator('.stage-snapshot')).toContainText('Synthetic state 16');
  const firstSnapshot = await page.locator('.stage-snapshot').innerText();
  await page.locator('#diagnostic-search').fill(fixture.diagnosticStages![0]!.label);
  await expect(stages).toHaveCount(1);
  await stages.first().click();
  await expect(page.locator('.stage-snapshot')).toContainText('Synthetic state 1');
  const secondSnapshot = await page.locator('.stage-snapshot').innerText();
  expect(secondSnapshot).not.toBe(firstSnapshot);
});

test('generic layout boundaries hold for every fixture at the target viewports', async ({ page }) => {
  test.setTimeout(240_000);
  await page.goto('/?fixture=fixture%3Avoid');
  await waitForReady(page);
  for (const size of viewSizes) {
    await page.setViewportSize(size);
    for (const scenario of SCENARIOS) {
      await switchFixture(page, scenario.id);
      const layout = await page.evaluate(() => {
        const panels = [...document.querySelectorAll<HTMLElement>('[data-panel]:not([hidden])')]
          .filter((element) => getComputedStyle(element).display !== 'none')
          .map((element) => { const rect = element.getBoundingClientRect(); return { x: rect.x, y: rect.y, right: rect.right, bottom: rect.bottom, name: element.dataset.panel }; });
        const overlays = [...document.querySelectorAll<HTMLElement>('[role="dialog"]:not([hidden]), .provider-error:not([hidden])')]
          .filter((element) => getComputedStyle(element).display !== 'none')
          .map((element) => { const rect = element.getBoundingClientRect(); return { x: rect.x, y: rect.y, right: rect.right, bottom: rect.bottom }; });
        return { width: innerWidth, height: innerHeight, documentWidth: document.documentElement.scrollWidth, bodyWidth: document.body.scrollWidth, panels, overlays };
      });
      expect(layout.documentWidth, `${scenario.id} at ${size.width}`).toBeLessThanOrEqual(size.width);
      expect(layout.bodyWidth, `${scenario.id} at ${size.width}`).toBeLessThanOrEqual(size.width);
      for (const panel of layout.panels) {
        expect(panel.x, `${scenario.id} ${panel.name} left`).toBeGreaterThanOrEqual(-1);
        expect(panel.y, `${scenario.id} ${panel.name} top`).toBeGreaterThanOrEqual(-1);
        expect(panel.right, `${scenario.id} ${panel.name} right`).toBeLessThanOrEqual(size.width + 1);
        expect(panel.bottom, `${scenario.id} ${panel.name} bottom`).toBeLessThanOrEqual(size.height + 1);
      }
      for (let i = 0; i < layout.panels.length; i += 1) for (let j = i + 1; j < layout.panels.length; j += 1) {
        const left = layout.panels[i]!;
        const right = layout.panels[j]!;
        const overlapX = Math.min(left.right, right.right) - Math.max(left.x, right.x);
        const overlapY = Math.min(left.bottom, right.bottom) - Math.max(left.y, right.y);
        expect(overlapX > 0 && overlapY > 0, `${scenario.id} panels ${left.name}/${right.name} overlap`).toBe(false);
      }
      for (const overlay of layout.overlays) {
        expect(overlay.x).toBeGreaterThanOrEqual(-1);
        expect(overlay.y).toBeGreaterThanOrEqual(-1);
        expect(overlay.right).toBeLessThanOrEqual(size.width + 1);
        expect(overlay.bottom).toBeLessThanOrEqual(size.height + 1);
      }
    }
  }
});

test('keyboard-only view choice and point inspection keep focus visible', async ({ page }) => {
  await page.setViewportSize(viewSizes[1]!);
  await page.goto('/?fixture=fixture%3Anormal-surface');
  await waitForReady(page);
  const debug = page.getByTestId('debug-button');
  await debug.focus();
  await page.keyboard.press('Enter');
  const search = page.getByRole('searchbox', { name: 'Search views' });
  await expect(search).toBeFocused();
  await search.fill('Roughness');
  await page.keyboard.press('ArrowDown');
  await expect(page.locator(':focus')).toHaveAttribute('data-view-id');
  await page.keyboard.press('Enter');
  await expect(debug).toBeFocused();
  await waitForReady(page);
  await page.keyboard.press('Tab');
  const features = page.getByRole('button', { name: /Features/ });
  if (await features.isVisible()) {
    await expect(features).toBeFocused();
    await page.keyboard.press('Tab');
  }
  await expect(page.getByRole('button', { name: 'Inspect point' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Inspect point' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Inspect point' })).toBeFocused();
});

test('compact layout keeps a large inspection report and menu panels distinct', async ({ page }) => {
  await page.setViewportSize(viewSizes[2]!);
  await page.goto('/?fixture=fixture%3Apoint-heavy');
  await waitForReady(page);
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await expect(page.locator('#legend-panel')).toBeVisible();
  await assertPanelsDoNotOverlap(page);
  await page.keyboard.press('Escape');
  await switchFixture(page, 'fixture:view-catalog-extreme');
  await waitForReady(page);
  await page.getByTestId('debug-button').click();
  await assertOverlayWithinViewport(page, '#debug-menu');
});

test('provider colors are validated before use in continuous and categorical legends', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const browserErrors: string[] = [];
  page.on('pageerror', (error) => browserErrors.push(error.message));
  page.on('console', (message) => { if (message.type() === 'error') browserErrors.push(message.text()); });
  const categoryStyles = await page.locator('#legend-panel .category-swatch').evaluateAll((elements) => elements.map((element) => (element as HTMLElement).style.getPropertyValue('--swatch')));
  expect(categoryStyles.slice(0, 7)).toEqual(['', '', '', '', '', '', '']);
  expect(categoryStyles[7]).toMatch(/^rgb\(/);

  const categoryAudit = await page.locator('#legend-panel').evaluate((panel) => ({
    eventAttributes: [...panel.querySelectorAll('*')].flatMap((element) => [...element.attributes].filter(({ name }) => name.toLowerCase().startsWith('on')).map(({ name }) => name)),
    scripts: panel.querySelectorAll('script').length,
    injected: panel.querySelectorAll('#injected, img, iframe, svg script').length,
  }));
  expect(categoryAudit).toEqual({ eventAttributes: [], scripts: 0, injected: 0 });
  await page.locator('#legend-panel .category-swatch').evaluateAll((elements) => elements.forEach((element) => element.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))));
  expect(await page.evaluate(() => window.hostileColorExecuted)).toBe(false);

  await selectRegressionView(page, 1);
  const continuous = await page.locator('#legend-panel .legend-scale').evaluate((element) => ({
    style: (element as HTMLElement).style.getPropertyValue('--scale'),
    computed: getComputedStyle(element).backgroundImage,
  }));
  expect(continuous.style).toContain('rgb(85, 96, 106)');
  expect(continuous.style).toContain('rgb(169, 176, 181)');
  expect(continuous.style).toContain('rgb(238, 240, 236)');
  expect(continuous.computed).toContain('linear-gradient');
  for (const hostile of ['onmouseover', 'javascript:', '</div>', '<script>', 'calc(', 'red; background', 'var(']) {
    expect(continuous.style).not.toContain(hostile);
  }
  await page.locator('#legend-panel .legend-scale').dispatchEvent('mouseover');
  expect(await page.evaluate(() => window.hostileColorExecuted)).toBe(false);
  const continuousAudit = await page.locator('#legend-panel').evaluate((panel) => ({
    eventAttributes: [...panel.querySelectorAll('*')].flatMap((element) => [...element.attributes].filter(({ name }) => name.toLowerCase().startsWith('on')).map(({ name }) => name)),
    scripts: panel.querySelectorAll('script').length,
  }));
  expect(continuousAudit).toEqual({ eventAttributes: [], scripts: 0 });
  await page.getByRole('button', { name: /Features/ }).click();
  const overlay = page.locator('#features-menu input[data-feature-id]').first();
  await overlay.check();
  await expect(overlay).toBeChecked();
  expect(browserErrors).toEqual([]);
});

test('normalizes CSS color syntaxes once for legend and viewport rendering', async ({ page }, testInfo) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const validColors = ['#ff0000', '#f00', 'hsl(0 100% 50%)', 'rgb(255 0 0)', 'red'];
  const normalized = await page.evaluate((colors) => colors.map((color) => window.remediationControl.normalizeColor(color)), validColors);
  expect(normalized.map((color) => color?.rgba)).toEqual(validColors.map(() => [255, 0, 0, 255]));
  expect(normalized.map((color) => color?.hex)).toEqual(validColors.map(() => '#ff0000'));
  const rendererSamples = await page.evaluate(() => [
    window.remediationControl.samplePalette([{ at: 0, color: 'rgb(255 0 0)' }, { at: 1, color: 'blue' }], 0),
    window.remediationControl.samplePalette([{ at: 0, color: 'rgb(255 0 0)' }, { at: 1, color: 'blue' }], 0.5),
    window.remediationControl.samplePalette([{ at: 0, color: 'rgb(255 0 0)' }, { at: 1, color: 'blue' }], 1),
  ]);
  expect(rendererSamples.map((color) => color.rgba)).toEqual([[255, 0, 0, 255], [128, 0, 128, 255], [0, 0, 255, 255]]);
  const hostile = [
    '#fff" onmouseover="alert(1)',
    'red; background:url(javascript:alert(1))',
    '</div><script>alert(1)</script>',
    'rgb(1 2 / calc(',
    '',
    'var(--injected, url(javascript:alert(1)))',
    '#fff" onmouseover="window.hostileColorExecuted=true',
    `rgb(${Array.from({ length: 80 }, () => '255').join(' ')})`,
    'rgb(255 0 0)\n',
    'rgba(255 0 0 / 0.5)',
  ];
  expect(await page.evaluate((colors) => colors.map((color) => window.remediationControl.normalizeColor(color)), hostile)).toEqual(hostile.map(() => null));

  await startRegressionHarness(page, 'fixture:category-heavy');
  const rgbView = await selectRegressionView(page, 2);
  const rgbLegend = await page.locator('#legend-panel .legend-scale').evaluate((element) => (element as HTMLElement).style.getPropertyValue('--scale'));
  expect(rgbLegend).toContain('rgb(255, 0, 0)');
  expect(rgbLegend).toContain('rgb(0, 0, 255)');
  await page.locator('#viewport canvas').screenshot({ path: testInfo.outputPath('palette-rgb-hsl-blue.png') });

  const hexNamedView = await selectRegressionView(page, 3);
  expect(hexNamedView).not.toBe(rgbView);
  const hexNamedLegend = await page.locator('#legend-panel .legend-scale').evaluate((element) => (element as HTMLElement).style.getPropertyValue('--scale'));
  expect(hexNamedLegend).toContain('rgb(255, 0, 0)');
  await page.locator('#viewport canvas').screenshot({ path: testInfo.outputPath('palette-hex-shorthex-named.png') });
});

test('captures regular and irregular rendered surfaces after the winding correction', async ({ page }, testInfo) => {
  for (const fixtureId of ['fixture:normal-surface', 'fixture:irregular']) {
    await page.goto(`/?fixture=${encodeURIComponent(fixtureId)}`);
    await waitForReady(page);
    await page.locator('#viewport canvas').screenshot({ path: testInfo.outputPath(`${fixtureId.slice('fixture:'.length)}.png`) });
  }
});

test('Inspect point follows the visible surface center ray before and after orbiting', async ({ page }, testInfo) => {
  await startRegressionHarness(page, 'fixture:normal-surface');
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  const unrotatedKey = await page.evaluate(() => window.remediationControl.inspectionPositionKey(0));
  if (!unrotatedKey) throw new Error('Unrotated center inspection did not record a position');
  const unrotated = JSON.parse(unrotatedKey) as { kind: string; direction: number[] };
  expect(unrotated.kind).toBe('surface-direction');
  expect(unrotated.direction[0]).toBeCloseTo(0, 5);
  expect(unrotated.direction[1]).toBeCloseTo(0, 5);
  expect(unrotated.direction[2]).toBeGreaterThan(0.98);

  const canvas = page.getByTestId('viewport-canvas');
  const bounds = await canvas.boundingBox();
  expect(bounds).not.toBeNull();
  const centerX = bounds!.x + bounds!.width / 2;
  const centerY = bounds!.y + bounds!.height / 2;
  await page.mouse.move(centerX, centerY);
  await page.mouse.down();
  await page.mouse.move(centerX + 150, centerY + 25, { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(1500);

  await canvas.click({ position: { x: bounds!.width / 2, y: bounds!.height / 2 } });
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(2);
  const clickedCenter = await page.evaluate(() => window.remediationControl.inspectionPositionKey(1));
  if (!clickedCenter) throw new Error('Center canvas pick did not record a position');
  const clicked = JSON.parse(clickedCenter) as { kind: string; direction: number[] };

  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(3);
  const buttonCenter = await page.evaluate(() => window.remediationControl.inspectionPositionKey(2));
  if (!buttonCenter) throw new Error('Center button pick did not record a position');
  const rotated = JSON.parse(buttonCenter) as { kind: string; direction: number[] };
  expect(rotated.kind).toBe('surface-direction');
  expect(Math.hypot(...rotated.direction.map((value, index) => value - clicked.direction[index]!))).toBeLessThan(0.05);
  expect(rotated.direction[2]).toBeLessThan(unrotated.direction[2]! - 0.1);
  await page.evaluate(() => window.remediationControl.resolveInspection(2, 'Visible center after orbit'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Visible center after orbit');
  await page.screenshot({ path: testInfo.outputPath('rotated-center-inspection.png') });
});

test('radial Inspect point selects the profile center and a missing surface hit selects nothing', async ({ page }, testInfo) => {
  await startRegressionHarness(page, 'fixture:radial');
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  expect(await page.evaluate(() => window.remediationControl.inspectionPositionKey(0))).toBe('{"kind":"radial-distance","normalizedRadius":0}');
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Radial center'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Radial center');
  await page.screenshot({ path: testInfo.outputPath('radial-center-inspection.png') });

  await startRegressionHarness(page, 'fixture:void');
  await page.getByRole('button', { name: 'Inspect point' }).click();
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(0);
  await expect(page.locator('#inspection-panel')).toBeHidden();
});

test('large radial profiles render with bounded SVG output and original sample counts', async ({ page }, testInfo) => {
  await page.goto('/?fixture=fixture%3Aradial');
  await waitForReady(page);
  await expect(page.locator('.profile-svg')).toBeVisible();
  await expect(page.locator('.profile-svg')).toHaveAttribute('data-source-sample-count', '64');
  await page.locator('#legend-panel').screenshot({ path: testInfo.outputPath('radial-profile-normal.png') });
  await page.locator('#viewport canvas').screenshot({ path: testInfo.outputPath('radial-profile-normal-viewport.png') });

  await startRegressionHarness(page, 'fixture:radial', 250_000);
  await expect(page.locator('#provider-state')).toHaveAttribute('data-state', 'ready');
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#viewport canvas')).toBeVisible();
  const chart = page.locator('.profile-svg');
  await expect(chart).toBeVisible();
  await expect(chart).toHaveAttribute('data-source-sample-count', '250000');
  await expect(page.locator('.profile-chart .eyebrow')).toContainText('250,000 samples');
  const plotted = await chart.locator('polyline').getAttribute('points');
  expect(plotted).not.toBeNull();
  expect(plotted!.split(' ')).toHaveLength(MAX_PROFILE_PLOT_POINTS);
  expect(plotted!.length).toBeLessThan(MAX_PROFILE_PLOT_POINTS * 16);
  expect(plotted).not.toMatch(/NaN|Infinity/);
  await page.locator('#legend-panel').screenshot({ path: testInfo.outputPath('radial-profile-large.png') });
  await page.locator('#viewport canvas').screenshot({ path: testInfo.outputPath('radial-profile-large-viewport.png') });
});

test('stale diagnostic stages cannot replace the selected snapshot or reappear after failure', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:diagnostics');
  await page.evaluate(() => window.remediationControl.setDiagnosticStageDeferred(true));
  const stageA = await selectRegressionStage(page, 0);
  const stageB = await selectRegressionStage(page, 1);
  await expect.poll(() => page.evaluate((id) => window.remediationControl.diagnosticStageCount(id), stageA)).toBe(1);
  await expect.poll(() => page.evaluate((id) => window.remediationControl.diagnosticStageCount(id), stageB)).toBe(1);

  await page.evaluate((id) => window.remediationControl.resolveStage(id, 'A stale'), stageA);
  await page.evaluate((id) => window.remediationControl.rejectStage(id, 'B failed'), stageB);
  await expect(page.locator('#provider-error')).toBeVisible();
  await expect(page.locator('#diagnostics-menu')).toBeHidden();
  await page.getByRole('button', { name: /Diagnostics/ }).click();
  await expect(page.locator('#diagnostics-menu .stage-snapshot')).toHaveCount(0);
  await expect(page.locator(`#diagnostics-menu [data-stage-id="${stageB}"]`)).toHaveAttribute('aria-selected', 'true');

  await startRegressionHarness(page, 'fixture:diagnostics');
  await page.evaluate(() => window.remediationControl.setDiagnosticStageDeferred(true));
  const olderStage = await selectRegressionStage(page, 0);
  const currentStage = await selectRegressionStage(page, 1);
  await page.evaluate((id) => window.remediationControl.resolveStage(id, 'B current'), currentStage);
  await expect(page.locator('.stage-snapshot')).toContainText('B current');
  await page.evaluate((id) => window.remediationControl.resolveStage(id, 'A stale'), olderStage);
  await expect(page.locator('.stage-snapshot')).toContainText('B current');
  await expect(page.locator('.stage-snapshot')).not.toContainText('A stale');
});

test('fixture-open retry repeats opening that fixture', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await page.evaluate(() => window.remediationControl.failNext('open', 'fixture:multi-domain'));
  await page.locator('#fixture-selector').selectOption('fixture:multi-domain');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  expect(await page.evaluate(() => window.remediationControl.operationCount('open', 'fixture:multi-domain'))).toBe(1);
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForReady(page);
  expect(await page.evaluate(() => window.remediationControl.operationCount('open', 'fixture:multi-domain'))).toBe(2);
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#body-name')).toHaveText('Multiple domain fixture');
});

test('metadata retry repeats only the failed provider request', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await page.evaluate(() => window.remediationControl.failNext('summary', 'fixture:multi-domain'));
  await page.locator('#fixture-selector').selectOption('fixture:multi-domain');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  expect(await page.evaluate(() => window.remediationControl.operationCount('open', 'fixture:multi-domain'))).toBe(1);
  expect(await page.evaluate(() => window.remediationControl.operationCount('summary', 'fixture:multi-domain'))).toBe(1);

  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForReady(page);
  expect(await page.evaluate(() => window.remediationControl.operationCount('open', 'fixture:multi-domain'))).toBe(1);
  expect(await page.evaluate(() => window.remediationControl.operationCount('summary', 'fixture:multi-domain'))).toBe(2);
  await expect(page.locator('#provider-error')).toBeHidden();
});

test('domain load retry preserves and completes the requested domain', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:multi-domain');
  await page.evaluate(() => window.remediationControl.failNext('views', 'domain-profile'));
  await page.locator('#domain-selector').selectOption('domain-profile');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  await expect(page.locator('#domain-selector')).toHaveValue('domain-profile');
  expect(await page.evaluate(() => window.remediationControl.operationCount('views', 'domain-profile'))).toBe(1);

  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForReady(page);
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#domain-selector')).toHaveValue('domain-profile');
  expect(await page.evaluate(() => window.remediationControl.operationCount('views', 'domain-profile'))).toBe(2);

  await page.evaluate(() => window.remediationControl.failNext('geometry', 'domain-mesh'));
  await page.locator('#domain-selector').selectOption('domain-mesh');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForReady(page);
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#domain-selector')).toHaveValue('domain-mesh');
  expect(await page.evaluate(() => window.remediationControl.operationCount('geometry', 'domain-mesh'))).toBe(2);
});

test('view-load retry preserves the current domain, view, and time selection', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:multi-domain');
  await page.locator('#domain-selector').selectOption('domain-profile');
  await waitForReady(page);
  const selectedView = await selectRegressionView(page, 1);
  await page.locator('#time-selector').selectOption('mean');
  await waitForReady(page);
  await page.evaluate((id) => window.remediationControl.failNext('tile', id), selectedView);
  await page.locator('#time-selector').selectOption('slice-2');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  await expect(page.locator('#domain-selector')).toHaveValue('domain-profile');
  await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', selectedView);
  await expect(page.locator('#time-selector')).toHaveValue('slice-2');

  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForReady(page);
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#domain-selector')).toHaveValue('domain-profile');
  await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', selectedView);
  await expect(page.locator('#time-selector')).toHaveValue('slice-2');
});

test('changing view invalidates a stale view retry action', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:multi-domain');
  await page.getByTestId('debug-button').click();
  const target = page.locator('#debug-menu [data-view-id]').nth(1);
  const targetView = await target.getAttribute('data-view-id');
  if (!targetView) throw new Error('Target view id is missing');
  await page.evaluate((id) => window.remediationControl.failNext('tile', id), targetView);
  await target.click();
  await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', targetView);
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

  await selectRegressionView(page, 0);
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
});

test('diagnostic-stage retry repeats the selected stage', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:diagnostics');
  await page.evaluate(() => window.remediationControl.failNext('stage', 'stage-3'));
  const stage = await selectRegressionStage(page, 2);
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  await expect(page.locator(`#diagnostics-menu [data-stage-id="${stage}"]`)).toHaveAttribute('aria-selected', 'true');

  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForReady(page);
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#diagnostics-menu')).toBeVisible();
  await expect(page.locator(`#diagnostics-menu [data-stage-id="${stage}"]`)).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('.stage-snapshot')).toContainText('Synthetic state 3');
});

test('inspection and explanation retry repeat their own point requests', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewId = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewId) throw new Error('Initial view id is missing');
  await page.evaluate(({ id }) => {
    window.remediationControl.failNext('inspect', '*');
    window.remediationControl.failNext('explain', id);
  }, { id: viewId });

  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 0, 1] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
  await page.getByRole('button', { name: 'Retry' }).click();
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(2);
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Retried point'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Retried point');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');

  await page.getByRole('button', { name: 'Retry' }).click();
  await waitForExplainCount(page, viewId, 'fixture:category-heavy', 2);
  await page.evaluate(({ id }) => window.remediationControl.resolveExplain(id, 'Retried explanation'), { id: viewId });
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('.explain-tree')).toContainText('Retried explanation');
});

test('non-retryable inspection rejection ends loading and closes as an inspection-owned failure', async ({ page }, testInfo) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 0, 1] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  await page.evaluate(() => window.remediationControl.rejectInspection(0, 'Point details are unavailable.'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Point inspection failed');
  await expect(page.locator('#inspection-panel')).toContainText('Point details are unavailable.');
  await expect(page.locator('#inspection-panel')).not.toContainText('Loading response');
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_INSPECT_TEST');
  await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath('inspection-failure.png') });
  await page.locator('.close-inspection').click();
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await expect(page.locator('#provider-error')).toBeHidden();
});

test('retryable inspection rejection ends loading and a successful retry restores the report', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await page.evaluate(() => window.remediationControl.failNext('inspect', '*'));
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 0, 1] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  await expect(page.locator('#inspection-panel h2')).toHaveText('Point inspection failed');
  await expect(page.locator('#inspection-panel')).not.toContainText('Loading response');
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

  await page.getByRole('button', { name: 'Retry' }).click();
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(2);
  await expect(page.locator('#inspection-panel')).toContainText('Loading response');
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Recovered point'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Recovered point');
  await expect(page.locator('#provider-error')).toBeHidden();
});

for (const failureMode of ['retryable', 'non-retryable'] as const) {
  test(`${failureMode} inspection failure is retired by a view change without stealing the view retry`, async ({ page }, testInfo) => {
    await openFailedInspection(page, 'fixture:category-heavy', failureMode);
    if (failureMode === 'retryable') await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
    else await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath('inspection-failure-before-view-change.png') });

    await page.getByTestId('debug-button').click();
    const target = page.locator('#debug-menu [data-view-id]').nth(1);
    const targetViewId = await target.getAttribute('data-view-id');
    if (!targetViewId) throw new Error('Target view identifier is missing');
    await page.evaluate((id) => window.remediationControl.failNext('tile', id), targetViewId);
    await target.click();

    await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', targetViewId);
    await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
    await expect(page.locator('#provider-error .error-message')).toHaveText('Temporary view data failure.');
    await expect(page.locator('#inspection-panel')).toBeHidden();
    await expect(page.locator('#provider-state')).toHaveAttribute('data-state', 'error');
    await page.screenshot({ path: testInfo.outputPath('view-failure-after-retirement.png') });

    await page.getByRole('button', { name: 'Retry' }).click();
    await waitForReady(page);
    await expect(page.locator('#provider-error')).toBeHidden();
    await expect(page.locator('#inspection-panel')).toBeHidden();
    await expect.poll(() => page.evaluate((id) => window.remediationControl.operationCount('tile', id), targetViewId)).toBe(2);
    await page.screenshot({ path: testInfo.outputPath('view-recovered-after-retirement.png') });
  });
}

test('failed inspection is retired when changing time', async ({ page }) => {
  await openFailedInspection(page, 'fixture:multi-domain', 'non-retryable');
  await expect(page.locator('#inspection-panel h2')).toHaveText('Point inspection failed');
  await page.locator('#time-selector').selectOption('slice-2');
  await waitForReady(page);
  await expect(page.locator('#time-selector')).toHaveValue('slice-2');
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await expect(page.locator('#provider-error')).toBeHidden();
});

test('failed inspection is retired when changing diagnostic stage', async ({ page }) => {
  await openFailedInspection(page, 'fixture:diagnostics', 'non-retryable');
  const stage = await selectRegressionStage(page, 1);
  await waitForReady(page);
  await expect(page.locator(`#diagnostics-menu [data-stage-id="${stage}"]`)).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await expect(page.locator('#provider-error')).toBeHidden();
});

test('late inspection completion after a view transition cannot reopen the retired panel', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 0, 1] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  await expect(page.locator('#inspection-panel')).toContainText('Loading response');

  await selectRegressionView(page, 1);
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Late retired response'));
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await expect(page.locator('#provider-error')).toBeHidden();
});

test('completed point reports remain available across view, time, and stage transitions', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Completed view report');
  await selectRegressionView(page, 1);
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await expect(page.locator('#inspection-panel h2')).toHaveText('Completed view report');

  await startRegressionHarness(page, 'fixture:multi-domain');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Completed time report');
  await page.locator('#time-selector').selectOption('slice-2');
  await waitForReady(page);
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await expect(page.locator('#inspection-panel h2')).toHaveText('Completed time report');

  await startRegressionHarness(page, 'fixture:diagnostics');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Completed stage report');
  await selectRegressionStage(page, 0);
  await waitForReady(page);
  await expect(page.locator('#inspection-panel')).toBeVisible();
  await expect(page.locator('#inspection-panel h2')).toHaveText('Completed stage report');
});

for (const closeMethod of ['button', 'escape'] as const) {
  test(`closing inspection with ${closeMethod} preserves an unrelated view-load failure`, async ({ page }, testInfo) => {
    await openRetryableViewFailureWithInspection(page);
    await expect(page.locator('#inspection-panel')).toBeVisible();
    await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('view-failure-with-inspection-open.png') });

    if (closeMethod === 'button') {
      await page.locator('.close-inspection').click();
    } else {
      await page.locator('#debug-button').focus();
      await page.keyboard.press('Escape');
    }

    await expect(page.locator('#inspection-panel')).toBeHidden();
    await expect(page.locator('#provider-error')).toBeVisible();
    await expect(page.locator('#provider-error .error-code')).toHaveText('E_RETRYABLE_TEST');
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('#provider-state')).toHaveAttribute('data-state', 'error');
    await page.screenshot({ path: testInfo.outputPath('view-failure-after-inspection-close.png') });

    await page.getByRole('button', { name: 'Retry' }).click();
    await waitForReady(page);
    await expect(page.locator('#provider-error')).toBeHidden();
  });
}

test('late explanation from view A cannot replace pending view B', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewA = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewA) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Point A');
  await waitForExplainCount(page, viewA, 'fixture:category-heavy', 1);

  const viewB = await selectRegressionView(page, 1);
  await waitForExplainCount(page, viewB, 'fixture:category-heavy', 1);
  await expect(page.locator('.explain-tree')).toContainText('Loading explanation');
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'A result'), { viewId: viewA });
  await expect(page.locator('.explain-tree')).not.toContainText('A result');
  await expect(page.locator('#provider-error')).toBeHidden();

  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'B result'), { viewId: viewB });
  await expect(page.locator('.explain-tree')).toContainText('B result');
});

test('view B remains current when it resolves before stale view A', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewA = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewA) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Point A');
  await waitForExplainCount(page, viewA, 'fixture:category-heavy', 1);
  const viewB = await selectRegressionView(page, 1);
  await waitForExplainCount(page, viewB, 'fixture:category-heavy', 1);

  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'B result'), { viewId: viewB });
  await expect(page.locator('.explain-tree')).toContainText('B result');
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'A result'), { viewId: viewA });
  await expect(page.locator('.explain-tree')).toContainText('B result');
  await expect(page.locator('.explain-tree')).not.toContainText('A result');
});

test('explanation snapshots clear immediately when the active view changes', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewA = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewA) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Point A');
  await waitForExplainCount(page, viewA, 'fixture:category-heavy', 1);
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'A snapshot'), { viewId: viewA });
  await expect(page.locator('.explain-tree')).toContainText('A snapshot');

  const viewB = await selectRegressionView(page, 1);
  await waitForExplainCount(page, viewB, 'fixture:category-heavy', 1);
  await expect(page.locator('.explain-tree')).toContainText('Loading explanation');
  await expect(page.locator('.explain-tree')).not.toContainText('A snapshot');
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'B current'), { viewId: viewB });
  await expect(page.locator('.explain-tree')).toContainText('B current');
});

test('rapid view A to B to C ignores both older successful explanations', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewA = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewA) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Point A');
  await waitForExplainCount(page, viewA, 'fixture:category-heavy', 1);
  const viewB = await selectRegressionView(page, 1);
  await waitForExplainCount(page, viewB, 'fixture:category-heavy', 1);
  const viewC = await selectRegressionView(page, 2);
  await waitForExplainCount(page, viewC, 'fixture:category-heavy', 1);

  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'A stale'), { viewId: viewA });
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'B stale'), { viewId: viewB });
  await expect(page.locator('.explain-tree')).not.toContainText('A stale');
  await expect(page.locator('.explain-tree')).not.toContainText('B stale');
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'C current'), { viewId: viewC });
  await expect(page.locator('.explain-tree')).toContainText('C current');
});

test('new point selection invalidates its old explanation and stale inspection reports', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewId = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewId) throw new Error('Initial view id is missing');
  const pointA: { kind: 'surface-direction'; direction: readonly [number, number, number] } = { kind: 'surface-direction', direction: [0, 0, 1] };
  const pointB: { kind: 'surface-direction'; direction: readonly [number, number, number] } = { kind: 'surface-direction', direction: [1, 0, 0] };
  await pickRegressionPoint(page, pointA, 'Point A');
  await waitForExplainCount(page, viewId, 'fixture:category-heavy', 1);

  const priorInspectionCount = await page.evaluate(() => window.remediationControl.inspectionCount());
  await page.evaluate((position) => window.remediationControl.pickPoint(position), pointB);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(priorInspectionCount + 1);
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Point B'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Point B');
  await waitForExplainCount(page, viewId, 'fixture:category-heavy', 2);
  expect(await page.evaluate(() => window.remediationControl.inspectionPositionKey(1))).not.toBe(await page.evaluate(() => window.remediationControl.inspectionPositionKey(0)));

  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'A stale'), { viewId });
  await expect(page.locator('.explain-tree')).not.toContainText('A stale');
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'B current'), { viewId });
  await expect(page.locator('.explain-tree')).toContainText('B current');

  const priorCount = await page.evaluate(() => window.remediationControl.inspectionCount());
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 1, 0] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(priorCount + 1);
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [-1, 0, 0] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(priorCount + 2);
  await page.evaluate(() => window.remediationControl.resolveInspection(1, 'Newest point'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Newest point');
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Stale point'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Newest point');
  const newestPositionKey = await page.evaluate(() => window.remediationControl.inspectionPositionKey(3));
  await waitForExplainCount(page, viewId, 'fixture:category-heavy', 3);
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'Newest explanation'), { viewId });
  await expect(page.locator('.explain-tree')).toContainText('Newest explanation');
  expect(newestPositionKey).toContain('[-1,0,0]');

  const failureRaceStart = await page.evaluate(() => window.remediationControl.inspectionCount());
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, -1, 0] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(failureRaceStart + 1);
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 0, -1] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(failureRaceStart + 2);
  await page.evaluate(() => window.remediationControl.rejectInspection(0, 'stale inspection failure'));
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#inspection-panel')).toContainText('Loading response');
  await page.evaluate(() => window.remediationControl.resolveInspection(0, 'Current inspection'));
  await expect(page.locator('#inspection-panel h2')).toHaveText('Current inspection');

  const currentCount = await page.evaluate(() => window.remediationControl.inspectionCount());
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, -1, 0] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(currentCount + 1);
  await page.evaluate(() => window.remediationControl.rejectInspection(0, 'current inspection failure'));
  await expect(page.locator('#provider-error')).toBeVisible();
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_INSPECT_TEST');
});

test('domain and fixture changes invalidate explanation context and stale failures', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:multi-domain');
  const firstView = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!firstView) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Domain point');
  await waitForExplainCount(page, firstView, 'fixture:multi-domain', 1);
  await page.locator('#domain-selector').selectOption('domain-profile');
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await page.evaluate(({ viewId }) => window.remediationControl.rejectExplain(viewId, 'stale domain failure', 'fixture:multi-domain'), { viewId: firstView });
  await expect(page.locator('#provider-error')).toBeHidden();

  await startRegressionHarness(page, 'fixture:category-heavy');
  const oldView = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!oldView) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Fixture point');
  await waitForExplainCount(page, oldView, 'fixture:category-heavy', 1);
  await page.locator('#fixture-selector').selectOption('fixture:multi-domain');
  await waitForReady(page);
  await expect(page.locator('#inspection-panel')).toBeHidden();
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'stale fixture result', 'fixture:category-heavy'), { viewId: oldView });
  await expect(page.locator('#provider-error')).toBeHidden();
  await expect(page.locator('#inspection-panel')).toBeHidden();
});

test('stale explanation failures are ignored while current failures still surface', async ({ page }) => {
  await startRegressionHarness(page, 'fixture:category-heavy');
  const viewA = await page.locator('#legend-panel').getAttribute('data-view-id');
  if (!viewA) throw new Error('Initial view id is missing');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Point A');
  await waitForExplainCount(page, viewA, 'fixture:category-heavy', 1);
  const viewB = await selectRegressionView(page, 1);
  await waitForExplainCount(page, viewB, 'fixture:category-heavy', 1);
  await page.evaluate(({ viewId }) => window.remediationControl.rejectExplain(viewId, 'stale explanation failure'), { viewId: viewA });
  await expect(page.locator('#provider-error')).toBeHidden();
  await page.evaluate(({ viewId }) => window.remediationControl.resolveExplain(viewId, 'B result'), { viewId: viewB });
  await expect(page.locator('.explain-tree')).toContainText('B result');

  const viewC = await selectRegressionView(page, 2);
  await waitForExplainCount(page, viewC, 'fixture:category-heavy', 1);
  await page.evaluate(({ viewId }) => window.remediationControl.rejectExplain(viewId, 'current explanation failure'), { viewId: viewC });
  await expect(page.locator('#provider-error')).toBeVisible();
  await expect(page.locator('#provider-error .error-code')).toHaveText('E_EXPLAIN_TEST');
  await expect(page.locator('.explain-error')).toHaveText('current explanation failure');
});

async function startRegressionHarness(page: Page, fixtureId: string, profileSamples?: number): Promise<void> {
  await page.setViewportSize(viewSizes[1]!);
  await page.addInitScript(() => { window.hostileColorExecuted = false; });
  const profileQuery = profileSamples === undefined ? '' : `&profileSamples=${profileSamples}`;
  await page.goto(`/tests/remediation-harness.html?fixture=${encodeURIComponent(fixtureId)}${profileQuery}`);
  await waitForReady(page);
}

async function pickRegressionPoint(
  page: Page,
  position: { kind: 'surface-direction'; direction: readonly [number, number, number] },
  label: string,
): Promise<void> {
  const previousCount = await page.evaluate(() => window.remediationControl.inspectionCount());
  await page.evaluate((point) => window.remediationControl.pickPoint(point), position);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(previousCount + 1);
  await page.evaluate(({ pointLabel }) => window.remediationControl.resolveInspection(0, pointLabel), { pointLabel: label });
  await expect(page.locator('#inspection-panel h2')).toHaveText(label);
}

async function selectRegressionView(page: Page, index: number): Promise<string> {
  await page.getByTestId('debug-button').click();
  const option = page.locator('#debug-menu [data-view-id]').nth(index);
  const viewId = await option.getAttribute('data-view-id');
  if (!viewId) throw new Error(`View option ${index} has no identifier`);
  await option.click();
  await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', viewId);
  await waitForReady(page);
  return viewId;
}

async function selectRegressionStage(page: Page, index: number): Promise<string> {
  if (await page.locator('#diagnostics-menu').isHidden()) await page.getByRole('button', { name: /Diagnostics/ }).click();
  const option = page.locator('#diagnostics-menu [data-stage-id]').nth(index);
  const stageId = await option.getAttribute('data-stage-id');
  if (!stageId) throw new Error(`Diagnostic stage ${index} has no identifier`);
  await option.click();
  return stageId;
}

async function openRetryableViewFailureWithInspection(page: Page): Promise<void> {
  await startRegressionHarness(page, 'fixture:category-heavy');
  await pickRegressionPoint(page, { kind: 'surface-direction', direction: [0, 0, 1] }, 'Inspection before view failure');
  const target = page.locator('#debug-menu [data-view-id]').nth(1);
  await page.getByTestId('debug-button').click();
  const targetViewId = await target.getAttribute('data-view-id');
  if (!targetViewId) throw new Error('Target view identifier is missing');
  await page.evaluate((id) => window.remediationControl.failNext('tile', id), targetViewId);
  await target.click();
  await expect(page.locator('#legend-panel')).toHaveAttribute('data-view-id', targetViewId);
  await expect(page.locator('#provider-error')).toBeVisible();
  await expect(page.locator('#inspection-panel h2')).toHaveText('Inspection before view failure');
}

async function openFailedInspection(page: Page, fixtureId: string, failureMode: 'retryable' | 'non-retryable'): Promise<void> {
  await startRegressionHarness(page, fixtureId);
  if (failureMode === 'retryable') await page.evaluate(() => window.remediationControl.failNext('inspect', '*'));
  await page.evaluate((position) => window.remediationControl.pickPoint(position), { kind: 'surface-direction', direction: [0, 0, 1] } as const);
  await expect.poll(() => page.evaluate(() => window.remediationControl.inspectionCount())).toBe(1);
  if (failureMode === 'non-retryable') {
    await page.evaluate(() => window.remediationControl.rejectInspection(0, 'Point details are unavailable.'));
  }
  await expect(page.locator('#inspection-panel h2')).toHaveText('Point inspection failed');
}

async function waitForExplainCount(page: Page, viewId: string, fixtureId: string, count: number): Promise<void> {
  await expect.poll(() => page.evaluate(({ id, fixture }) => window.remediationControl.explanationCount(id, fixture), { id: viewId, fixture: fixtureId })).toBe(count);
}

async function waitForReady(page: Page): Promise<void> {
  await expect.poll(async () => await page.locator('#provider-state').getAttribute('data-state'), { timeout: 15_000 }).toBe('ready');
}

async function switchFixture(page: Page, fixtureId: string): Promise<void> {
  const selected = new URL(page.url()).searchParams.get('fixture');
  if (selected !== fixtureId) await page.locator('#fixture-selector').selectOption(fixtureId);
  await expect.poll(async () => await page.locator('#provider-state').getAttribute('data-state'), { timeout: 20_000 }).toMatch(/^(ready|error)$/);
}

async function assertPanelsDoNotOverlap(page: Page): Promise<void> {
  const panelBounds = await page.locator('[data-panel]:visible').evaluateAll((elements) => elements.map((element) => {
    const rect = element.getBoundingClientRect();
    return { x: rect.x, y: rect.y, right: rect.right, bottom: rect.bottom };
  }));
  for (let leftIndex = 0; leftIndex < panelBounds.length; leftIndex += 1) for (let rightIndex = leftIndex + 1; rightIndex < panelBounds.length; rightIndex += 1) {
    const left = panelBounds[leftIndex]!;
    const right = panelBounds[rightIndex]!;
    const overlapX = Math.min(left.right, right.right) - Math.max(left.x, right.x);
    const overlapY = Math.min(left.bottom, right.bottom) - Math.max(left.y, right.y);
    expect(overlapX > 0 && overlapY > 0).toBe(false);
  }
}

async function assertOverlayWithinViewport(page: Page, selector: string): Promise<void> {
  const bounds = await page.locator(selector).boundingBox();
  const viewport = page.viewportSize();
  expect(bounds).not.toBeNull();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(viewport!.width + 1);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport!.height + 1);
}
