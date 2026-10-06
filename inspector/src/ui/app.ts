import type {
  BodyCatalog,
  BodyProvider,
  BodySummary,
  ExplainStep,
  DiagnosticSnapshot,
  DiagnosticsDescriptor,
  DomainDescriptor,
  FeatureCatalog,
  FixtureOption,
  PickPosition,
  PointReport,
  ProviderFailure,
  RenderGeometry,
  ViewCatalog,
  ViewDescriptor,
  ViewStats,
} from '../provider/contracts';
import { asProviderFailure } from '../provider/contracts';
import { Viewport } from '../render/viewport';
import { orderedGroups } from './view-model';

type LoadState = 'opening' | 'metadata' | 'view' | 'ready' | 'error';

export class InspectorApp {
  private readonly viewport: Viewport;
  private readonly fixtures: readonly FixtureOption[];
  private fixtureId: string;
  private provider: BodyProvider | null = null;
  private summary: BodySummary | null = null;
  private domains: readonly DomainDescriptor[] = [];
  private activeDomain: DomainDescriptor | null = null;
  private catalog: ViewCatalog = { groups: [], views: [] };
  private activeView: ViewDescriptor | null = null;
  private activeTimeId: string | undefined;
  private activeStageId: string | undefined;
  private geometry: RenderGeometry | null = null;
  private stats: ViewStats | undefined;
  private diagnostics: DiagnosticsDescriptor = { available: false };
  private snapshot: DiagnosticSnapshot | null = null;
  private featureCatalog: FeatureCatalog = { tables: [] };
  private selectedOverlays = new Set<string>();
  private selectedPosition: PickPosition | null = null;
  private pointReport: PointReport | null = null;
  private failure: ProviderFailure | null = null;
  private loadState: LoadState = 'opening';
  private debugOpen = false;
  private diagnosticsOpen = false;
  private featuresOpen = false;
  private requestVersion = 0;

  constructor(private readonly root: HTMLDivElement, private readonly bodyCatalog: BodyCatalog) {
    this.fixtures = bodyCatalog.fixtures();
    const requested = new URL(window.location.href).searchParams.get('fixture');
    this.fixtureId = this.fixtures.some(({ id }) => id === requested) ? requested! : 'fixture:normal-surface';
    this.root.innerHTML = this.shell();
    this.viewport = new Viewport(this.element<HTMLDivElement>('#viewport'), (position) => void this.inspect(position));
    this.root.addEventListener('click', this.handleClick);
    this.root.addEventListener('change', this.handleChange);
    this.root.addEventListener('input', this.handleInput);
    this.root.addEventListener('keydown', this.handleKeydown);
  }

  async start(): Promise<void> {
    this.renderAll();
    await this.openFixture(this.fixtureId);
  }

  destroy(): void {
    this.root.removeEventListener('click', this.handleClick);
    this.root.removeEventListener('change', this.handleChange);
    this.root.removeEventListener('input', this.handleInput);
    this.root.removeEventListener('keydown', this.handleKeydown);
    this.viewport.destroy();
  }

  private shell(): string {
    return `<div class="app-shell">
      <header class="topbar">
        <a class="brand-lockup" href="#" aria-label="VEYRA Inspector"><span class="brand-mark" aria-hidden="true">V</span><span>VEYRA <b>Inspector</b></span></a>
        <div class="body-identity"><span id="body-name">Opening fixture…</span><span class="synthetic-tag">SYNTHETIC FIXTURE</span></div>
        <div class="toolbar-controls">
          <label class="control-label domain-control">Domain <select id="domain-selector" aria-label="Domain"></select></label>
          <button id="debug-button" class="toolbar-button" data-testid="debug-button" aria-haspopup="dialog" aria-expanded="false" aria-controls="debug-menu">Debug · …</button>
          <label id="temporal-control" class="control-label temporal-control" hidden>Time <select id="time-selector" aria-label="Time selection"></select></label>
          <button id="diagnostics-button" class="toolbar-button" aria-haspopup="dialog" aria-expanded="false" hidden>Diagnostics</button>
          <button id="features-button" class="toolbar-button" aria-haspopup="dialog" aria-expanded="false" hidden>Features</button>
          <button id="inspect-center" class="toolbar-button inspect-action" title="Inspect the center of the displayed model">Inspect point</button>
          <button id="settings-button" class="icon-button" aria-label="Display settings" title="Display settings">⋯</button>
        </div>
      </header>
      <main class="workspace">
        <div id="viewport" class="viewport" aria-label="Main interactive viewport"></div>
        <div class="viewport-hint" aria-hidden="true">Drag to rotate · Scroll to zoom · Select a point to inspect</div>
        <div id="provider-state" class="provider-state" role="status" aria-live="polite"></div>
        <section id="provider-error" class="provider-error" role="alert" hidden></section>
        <div class="right-rail">
          <section id="legend-panel" class="panel legend-panel" data-panel="legend" aria-labelledby="view-name"></section>
          <aside id="inspection-panel" class="panel inspection-panel" data-panel="inspection" aria-label="Point inspection" hidden></aside>
        </div>
        <section id="debug-menu" class="popover debug-menu" role="dialog" aria-label="View catalogue" hidden></section>
        <section id="diagnostics-menu" class="popover diagnostics-menu" role="dialog" aria-label="Diagnostics stages" hidden></section>
        <section id="features-menu" class="popover features-menu" role="dialog" aria-label="Feature overlays" hidden></section>
      </main>
      <footer class="status-strip">
        <span class="footer-fixture"><span class="status-dot"></span><label for="fixture-selector">Fixture</label><select id="fixture-selector" aria-label="Synthetic fixture"></select></span>
        <span id="status-body">BODY · —</span><span id="status-domain">DOMAIN · —</span>
        <span class="status-source">SOURCE · SYNTHETIC FIXTURE</span>
      </footer>
    </div>`;
  }

  private async openFixture(fixtureId: string): Promise<void> {
    const version = ++this.requestVersion;
    this.fixtureId = fixtureId;
    this.provider = null;
    this.summary = null;
    this.domains = [];
    this.activeDomain = null;
    this.catalog = { groups: [], views: [] };
    this.activeView = null;
    this.activeTimeId = undefined;
    this.activeStageId = undefined;
    this.geometry = null;
    this.stats = undefined;
    this.diagnostics = { available: false };
    this.snapshot = null;
    this.featureCatalog = { tables: [] };
    this.selectedOverlays.clear();
    this.selectedPosition = null;
    this.pointReport = null;
    this.failure = null;
    this.loadState = 'opening';
    this.setFixtureQuery(fixtureId);
    this.closePopovers(false);
    this.viewport.clearData();
    this.renderAll();
    try {
      const provider = await this.bodyCatalog.open(fixtureId);
      if (version !== this.requestVersion) return;
      this.provider = provider;
      this.loadState = 'metadata';
      this.renderStatus();
      const [summary, domains, diagnostics, featureCatalog] = await Promise.all([
        provider.summary(), provider.domains(), provider.diagnostics(), provider.features(),
      ]);
      if (version !== this.requestVersion) return;
      this.summary = summary;
      this.domains = domains;
      this.diagnostics = diagnostics;
      this.featureCatalog = featureCatalog;
      this.activeDomain = domains[0] ?? null;
      if (this.activeDomain) {
        this.catalog = await provider.views(this.activeDomain.id);
        this.activeView = this.catalog.views[0] ?? null;
        this.activeTimeId = this.defaultTime(this.activeView);
        this.geometry = await provider.domainGeometry(this.activeDomain.id);
      }
      this.loadState = 'ready';
      this.renderAll();
      if (this.activeView) await this.loadActiveView(version);
    } catch (error) {
      if (version !== this.requestVersion) return;
      this.showFailure(error);
    }
  }

  private async changeDomain(domainId: string): Promise<void> {
    const domain = this.domains.find(({ id }) => id === domainId);
    if (!domain || !this.provider) return;
    const version = ++this.requestVersion;
    this.activeDomain = domain;
    this.activeStageId = undefined;
    this.snapshot = null;
    this.pointReport = null;
    this.selectedPosition = null;
    this.geometry = null;
    this.stats = undefined;
    this.viewport.clearData();
    this.loadState = 'metadata';
    this.renderAll();
    try {
      const [catalog, geometry] = await Promise.all([this.provider.views(domain.id), this.provider.domainGeometry(domain.id)]);
      if (version !== this.requestVersion) return;
      this.catalog = catalog;
      this.geometry = geometry;
      this.activeView = catalog.views[0] ?? null;
      this.activeTimeId = this.defaultTime(this.activeView);
      this.loadState = 'ready';
      this.renderAll();
      if (this.activeView) await this.loadActiveView(version);
      this.element<HTMLSelectElement>('#domain-selector').focus();
    } catch (error) {
      if (version !== this.requestVersion) return;
      this.showFailure(error);
    }
  }

  private async selectView(viewId: string): Promise<void> {
    const view = this.catalog.views.find(({ id }) => id === viewId);
    if (!view || !this.provider) return;
    const version = ++this.requestVersion;
    this.activeView = view;
    this.activeTimeId = this.defaultTime(view);
    this.stats = undefined;
    this.closePopovers(true);
    this.loadState = 'view';
    this.failure = null;
    this.viewport.clearData();
    this.renderAll();
    await this.loadActiveView(version);
  }

  private async loadActiveView(version: number): Promise<void> {
    const provider = this.provider;
    const view = this.activeView;
    const domain = this.activeDomain;
    if (!provider || !view || !domain) {
      this.loadState = 'ready';
      this.renderAll();
      return;
    }
    this.loadState = 'view';
    this.failure = null;
    this.renderStatus();
    this.renderLegend();
    try {
      const [geometry, tile, stats] = await Promise.all([
        this.geometry ? Promise.resolve(this.geometry) : provider.domainGeometry(domain.id),
        provider.tile(view.id, this.activeTimeId, this.activeStageId),
        provider.stats(view.id),
      ]);
      if (version !== this.requestVersion) return;
      this.geometry = geometry;
      this.stats = stats;
      this.loadState = 'ready';
      this.viewport.setData(geometry, view, tile);
      this.viewport.setOverlays(this.featureCatalog.tables.filter((table) => this.selectedOverlays.has(table.id)));
      this.renderAll(tile.missingResources ?? []);
    } catch (error) {
      if (version !== this.requestVersion) return;
      this.viewport.clearData();
      this.showFailure(error);
    }
  }

  private async inspect(position: PickPosition): Promise<void> {
    if (!this.provider) return;
    this.selectedPosition = position;
    this.pointReport = null;
    this.renderInspection();
    try {
      const report = await this.provider.inspect(position);
      if (this.selectedPosition !== position) return;
      this.pointReport = report;
      this.renderInspection();
      const input = this.root.querySelector<HTMLInputElement>('#field-search');
      if (input) input.focus();
    } catch (error) {
      this.showFailure(error);
    }
  }

  private async selectStage(stageId: string): Promise<void> {
    if (!this.provider) return;
    this.activeStageId = stageId;
    this.snapshot = null;
    const version = ++this.requestVersion;
    this.renderDiagnostics();
    this.focusStage(stageId);
    try {
      this.snapshot = await this.provider.diagnosticStage(stageId);
      if (version !== this.requestVersion) return;
      this.renderDiagnostics();
      this.focusStage(stageId);
      if (this.activeView) await this.loadActiveView(version);
      this.focusStage(stageId);
    } catch (error) {
      if (version === this.requestVersion) this.showFailure(error);
    }
  }

  private async selectTime(timeId: string): Promise<void> {
    if (!this.activeView?.timeSelections?.some(({ id }) => id === timeId)) return;
    this.activeTimeId = timeId;
    const version = ++this.requestVersion;
    this.loadState = 'view';
    this.viewport.clearData();
    this.renderStatus();
    await this.loadActiveView(version);
    this.element<HTMLSelectElement>('#time-selector').focus();
  }

  private async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.setCopyStatus('Copied');
    } catch {
      this.setCopyStatus('Clipboard unavailable');
    }
  }

  private setCopyStatus(message: string): void {
    const output = this.root.querySelector<HTMLElement>('#copy-status');
    if (output) output.textContent = message;
  }

  private showFailure(error: unknown): void {
    this.failure = asProviderFailure(error);
    this.loadState = 'error';
    this.renderAll();
  }

  private renderAll(missingResources: readonly string[] = []): void {
    this.renderFixtureSelector();
    this.renderIdentity();
    this.renderDomainSelector();
    this.renderDebugButton();
    this.renderTemporalControl();
    this.renderDiagnosticsButton();
    this.renderFeaturesButton();
    this.renderStatus(missingResources);
    this.renderError();
    this.renderLegend(missingResources);
    this.renderInspection();
    this.renderDebugMenu();
    this.renderDiagnostics();
    this.renderFeatures();
  }

  private renderFixtureSelector(): void {
    const selector = this.element<HTMLSelectElement>('#fixture-selector');
    selector.innerHTML = this.fixtures.map((fixture) => `<option value="${attr(fixture.id)}" title="${attr(fixture.description)}">${escape(fixture.label)}</option>`).join('');
    selector.value = this.fixtureId;
  }

  private renderIdentity(): void {
    this.element<HTMLElement>('#body-name').textContent = this.summary?.name ?? this.currentFixture()?.label ?? 'Opening fixture…';
    this.element<HTMLElement>('#status-body').textContent = `BODY · ${this.summary ? shorten(this.summary.objectId) : '—'}`;
    this.element<HTMLElement>('#status-domain').textContent = this.activeDomain ? `DOMAIN · ${this.activeDomain.topology}` : 'DOMAIN · NONE';
  }

  private renderDomainSelector(): void {
    const selector = this.element<HTMLSelectElement>('#domain-selector');
    selector.innerHTML = this.domains.map((domain) => `<option value="${attr(domain.id)}">${escape(domain.label)} · ${escape(domain.topology)}</option>`).join('');
    selector.disabled = this.domains.length <= 1;
    selector.hidden = this.domains.length === 0;
    if (this.activeDomain) selector.value = this.activeDomain.id;
  }

  private renderDebugButton(): void {
    const button = this.element<HTMLButtonElement>('#debug-button');
    const count = this.catalog.views.length;
    button.disabled = count <= 1;
    button.setAttribute('aria-expanded', String(this.debugOpen && count > 1));
    button.textContent = count === 0 ? 'Debug · none' : count === 1 ? `View · ${this.activeView?.label ?? '1 view'}` : `Debug · ${count}`;
    button.title = count <= 1 ? (count === 0 ? 'This fixture declares no views.' : 'This fixture declares a single view; no catalogue is needed.') : 'Search the provider-declared view catalogue.';
  }

  private renderTemporalControl(): void {
    const wrap = this.element<HTMLLabelElement>('#temporal-control');
    const select = this.element<HTMLSelectElement>('#time-selector');
    const choices = this.activeView?.timeSelections ?? [];
    wrap.hidden = choices.length === 0;
    select.innerHTML = choices.map((item) => `<option value="${attr(item.id)}" title="${attr(item.label)}">${escape(item.label)}</option>`).join('');
    select.value = this.activeTimeId ?? choices[0]?.id ?? '';
  }

  private renderDiagnosticsButton(): void {
    const button = this.element<HTMLButtonElement>('#diagnostics-button');
    const stages = this.diagnostics.available ? this.diagnostics.stages : [];
    const available = stages.length > 0;
    button.hidden = !available;
    button.setAttribute('aria-expanded', String(this.diagnosticsOpen && available));
    button.textContent = available ? `Diagnostics · ${stages.length}` : 'Diagnostics';
  }

  private renderFeaturesButton(): void {
    const button = this.element<HTMLButtonElement>('#features-button');
    const available = this.featureCatalog.tables.some(({ geometry }) => geometry !== undefined);
    button.hidden = !available;
    button.setAttribute('aria-expanded', String(this.featuresOpen && available));
    button.textContent = available ? `Features · ${this.featureCatalog.tables.length}` : 'Features';
  }

  private renderStatus(missingResources: readonly string[] = []): void {
    const status = this.element<HTMLElement>('#provider-state');
    status.dataset.state = this.loadState;
    const messages: Record<LoadState, string> = {
      opening: 'Opening fixture…',
      metadata: 'Loading provider metadata…',
      view: 'Loading view data…',
      ready: '',
      error: '',
    };
    if (this.loadState === 'ready' && missingResources.length > 0) {
      status.textContent = `INCOMPLETE DATA · ${missingResources.length} missing resources · available values are shown`;
      status.hidden = false;
    } else {
      status.textContent = messages[this.loadState];
      status.hidden = status.textContent.length === 0;
    }
  }

  private renderError(): void {
    const panel = this.element<HTMLElement>('#provider-error');
    if (!this.failure) {
      panel.hidden = true;
      panel.innerHTML = '';
      return;
    }
    const failure = this.failure;
    panel.hidden = false;
    panel.innerHTML = `<div class="error-heading"><span class="error-mark" aria-hidden="true">!</span><div><p class="eyebrow">PROVIDER RESPONSE</p><h2>${escape(errorTitle(failure))}</h2></div></div>
      <code class="error-code">${escape(failure.code)}</code><p class="error-message">${escape(failure.message)}</p>
      ${failure.offendingItem ? `<p class="error-item">Item · <code title="${attr(failure.offendingItem)}">${escape(failure.offendingItem)}</code></p>` : ''}
      ${failure.retryable ? '<button id="retry-button" class="secondary-button">Retry</button>' : ''}`;
  }

  private renderLegend(missingResources: readonly string[] = []): void {
    const panel = this.element<HTMLElement>('#legend-panel');
    const view = this.activeView;
    if (!view) {
      const pending = this.loadState === 'opening' || this.loadState === 'metadata';
      const title = pending ? 'Waiting for metadata' : this.failure ? 'No view available' : 'No view declared';
      const description = pending
        ? 'The provider is opening this fixture and has not returned its descriptors yet.'
        : this.failure
          ? 'The provider did not return a view catalogue for this fixture.'
          : this.domains.length
            ? 'The provider returned no views for this domain.'
            : 'The provider returned no domains for this fixture.';
      panel.innerHTML = `<div class="panel-heading"><p class="eyebrow">CURRENT VIEW</p><h2 id="view-name">${title}</h2></div><p class="muted">${description}</p>`;
      return;
    }
    const group = this.catalog.groups.find(({ id }) => id === view.groupId);
    panel.innerHTML = `<div class="panel-heading"><div><p class="eyebrow">${escape(group?.label ?? 'View')}</p><h2 id="view-name" title="${attr(view.label)}">${escape(view.label)}</h2></div><span class="view-counter">${this.catalog.views.indexOf(view) + 1} / ${this.catalog.views.length}</span></div>
      <p id="view-description" class="view-description" title="${attr(view.description)}">${escape(view.description)}</p>
      <div class="legend-body">${this.renderLegendContent(view, this.stats)}</div>
      ${this.activeDomain?.renderKind === 'radial-profile' && this.geometry?.profile ? `<div class="profile-chart"><p class="eyebrow">PROFILE · PROVIDER VALUES</p>${profileSvg(this.geometry.profile)}</div>` : ''}
      ${missingResources.length ? `<div class="incomplete-note" role="status"><b>Incomplete response</b><span>${missingResources.map((item) => `<span title="${attr(item)}">${escape(item)}</span>`).join(', ')}</span></div>` : ''}
      <p class="legend-source">All values are synthetic fixture responses.</p>`;
    panel.dataset.viewId = view.id;
  }

  private renderLegendContent(view: ViewDescriptor, stats: ViewStats | undefined): string {
    const legend = view.legend;
    if (legend.kind === 'categorical') {
      return `<div class="legend-unit-row">${legend.unit ? `<span class="unit-tag">${escape(legend.unit)}</span>` : '<span class="muted">No unit provided</span>'}<span class="muted">${legend.categories.length} categories</span></div>
        <div class="category-list" role="list" aria-label="Legend categories">${legend.categories.map((category) => `<div class="category-row" role="listitem" tabindex="0" title="${attr(category.label)}"><span class="category-swatch" style="--swatch:${attr(category.color)}"></span><span class="category-name" title="${attr(category.label)}">${escape(category.label)}</span><span class="category-metrics">${category.count === undefined ? '—' : formatCount(category.count)}${category.weightedPercent === undefined ? '' : `<small>${formatPercent(category.weightedPercent)}</small>`}</span></div>`).join('')}</div>`;
    }
    const gradient = legend.stops.map(({ at, color }) => `${color} ${(at * 100).toFixed(0)}%`).join(', ');
    const statText = legend.statsAvailable && stats
      ? `<div class="stats-grid"><div><span>MIN</span><b title="${attr(String(stats.min))}">${formatNumber(stats.min)}</b></div><div><span>MAX</span><b title="${attr(String(stats.max))}">${formatNumber(stats.max)}</b></div><div><span>MEAN</span><b title="${attr(String(stats.mean))}">${formatNumber(stats.mean)}</b></div></div>${stats.count === undefined ? '' : `<p class="sample-count">${formatCount(stats.count)} provider records</p>`}`
      : '<p class="no-stats">No statistics available</p>';
    return `<div class="legend-unit-row">${legend.unit ? `<span class="unit-tag">${escape(legend.unit)}</span>` : '<span class="muted">No unit provided</span>'}${legend.range ? `<span class="muted range-value" title="${attr(`${legend.range.min} – ${legend.range.max}`)}">${formatNumber(legend.range.min)} – ${formatNumber(legend.range.max)}</span>` : ''}</div>
      <div class="legend-scale" role="img" aria-label="Continuous colour scale${legend.unit ? ` in ${attr(legend.unit)}` : ''}" style="--scale:linear-gradient(90deg, ${gradient})"></div>
      <div class="scale-ends"><span>${legend.range ? formatNumber(legend.range.min) : 'Low'}</span><span>${legend.range ? formatNumber(legend.range.max) : 'High'}</span></div>${statText}`;
  }

  private renderInspection(): void {
    const panel = this.element<HTMLElement>('#inspection-panel');
    if (!this.selectedPosition) {
      panel.hidden = true;
      panel.innerHTML = '';
      return;
    }
    panel.hidden = false;
    if (!this.pointReport) {
      panel.classList.remove('large-report');
      panel.innerHTML = '<div class="panel-heading"><h2>Point inspection</h2><span class="muted">Loading response…</span></div>';
      return;
    }
    const report = this.pointReport;
    const count = report.groups.reduce((total, group) => total + group.fields.length, 0);
    panel.classList.toggle('large-report', count > 12);
    const groups = report.groups.map((group, index) => `<details class="field-group" ${index < 2 ? 'open' : ''}>
      <summary><span title="${attr(group.label)}">${escape(group.label)}</span><small>${group.fields.length}</small></summary>
      <div class="field-list">${group.fields.map((field) => this.renderField(field, group.label)).join('')}</div>
    </details>`).join('');
    const explain = this.activeView && this.provider && this.selectedPosition
      ? `<details class="explain-block"><summary>Provider explanation</summary><div class="explain-tree">${this.renderExplain(this.snapshotExplain)}</div></details>`
      : '';
    panel.innerHTML = `<header class="inspection-header"><div><p class="eyebrow">SELECTED POSITION · ${count} FIELDS</p><h2 title="${attr(report.positionLabel)}">${escape(report.positionLabel)}</h2><code title="${attr(report.positionValue)}">${escape(report.positionValue)}</code></div><button class="icon-button close-inspection" aria-label="Close point inspection">×</button></header>
      ${count > 8 ? '<label class="search-wrap field-search-wrap"><span aria-hidden="true">⌕</span><input id="field-search" type="search" placeholder="Filter fields and groups" aria-label="Filter point fields"></label>' : ''}
      <div class="report-scroll"><div class="field-groups" id="field-groups">${groups || '<p class="muted">No fields supplied for this position.</p>'}</div>
      ${explain}<div class="copy-row"><button class="secondary-button copy-position">Copy position</button><span id="copy-status" class="sr-only" aria-live="polite"></span></div></div>`;
    void this.loadExplain();
  }

  private snapshotExplain: readonly ExplainStep[] = [];

  private async loadExplain(): Promise<void> {
    if (!this.provider || !this.activeView || !this.selectedPosition) return;
    const position = this.selectedPosition;
    try {
      const chain = await this.provider.explain(position, this.activeView.id);
      if (this.selectedPosition !== position) return;
      this.snapshotExplain = chain;
      const target = this.root.querySelector<HTMLElement>('.explain-tree');
      if (target) target.innerHTML = this.renderExplain(chain);
    } catch (error) {
      this.showFailure(error);
    }
  }

  private renderField(field: PointReport['groups'][number]['fields'][number], groupLabel: string): string {
    const value = field.nodata || field.value == null ? 'No data' : field.value;
    const unit = field.unit ? `<span class="field-unit">${escape(field.unit)}</span>` : '';
    const level = field.levelUsed == null ? 'Level not provided' : `Level ${escape(String(field.levelUsed))}`;
    const searchable = `${groupLabel} ${field.label} ${value} ${field.sourceKind} ${level} ${field.unit ?? ''}`.toLowerCase();
    return `<div class="field-row" data-field="${attr(field.id)}" data-search="${attr(searchable)}">
      <div class="field-copy"><span class="field-name" tabindex="0" aria-label="${attr(field.label)}" title="${attr(field.label)}">${escape(field.label)}</span><span class="field-source" title="Source ${attr(field.sourceKind)} · ${attr(level)}">${escape(field.sourceKind)} · ${level}</span></div>
      <div class="field-result"><span class="field-value" tabindex="0" aria-label="${attr(`${value}${field.unit ? ` ${field.unit}` : ''}`)}" title="${attr(`${value}${field.unit ? ` ${field.unit}` : ''}`)}">${escape(value)}</span>${unit}<button class="copy-value" data-value="${attr(value)}" data-field-label="${attr(field.label)}" aria-label="Copy ${attr(field.label)} value" title="Copy value">⧉</button></div>
    </div>`;
  }

  private renderExplain(nodes: readonly ExplainStep[]): string {
    return nodes.map((node) => `<details class="explain-step"><summary title="${attr(node.label)}">${escape(node.label)}</summary><p title="${attr(node.description)}">${escape(node.description)}</p>
      ${(node.references?.length ?? 0) ? `<dl class="reference-list">${node.references!.map((reference) => `<div><dt>${escape(reference.label)}</dt><dd title="${attr(reference.value)}">${escape(reference.value)}</dd></div>`).join('')}</dl>` : ''}
      ${(node.children?.length ?? 0) ? `<div class="explain-children">${this.renderExplain(node.children!)}</div>` : ''}</details>`).join('');
  }

  private renderDebugMenu(): void {
    const menu = this.element<HTMLElement>('#debug-menu');
    const button = this.element<HTMLButtonElement>('#debug-button');
    const enabled = this.debugOpen && this.catalog.views.length > 1;
    menu.hidden = !enabled;
    if (!enabled) return;
    const groups = orderedGroups(this.catalog);
    const records = groups.map(({ group, views }) => `<details class="catalog-group" data-group-id="${attr(group.id)}" open>
      <summary><span class="group-label" title="${attr(group.label)}">${escape(group.label)}</span><small>${views.length}</small></summary>
      <div class="catalog-options" role="group" aria-label="${attr(group.label)}">${views.map((view) => `<button class="view-option" role="option" aria-selected="${view.id === this.activeView?.id}" data-view-id="${attr(view.id)}" data-search="${attr(`${group.label} ${view.label} ${view.description}`.toLowerCase())}" title="${attr(view.label)}"><span class="option-label">${escape(view.label)}</span>${view.id === this.activeView?.id ? '<span class="current-mark">CURRENT</span>' : ''}</button>`).join('')}</div>
    </details>`).join('');
    menu.innerHTML = `<div class="popover-heading"><div><p class="eyebrow">PROVIDER CATALOGUE</p><h2>Debug views</h2></div><button class="icon-button close-popover" aria-label="Close view catalogue">×</button></div>
      <label class="search-wrap"><span aria-hidden="true">⌕</span><input id="debug-search" type="search" placeholder="Search ${this.catalog.views.length} views" aria-label="Search views" autocomplete="off"></label>
      <div id="view-catalog-list" class="catalog-scroll" role="listbox" aria-label="Declared view groups">${records}</div><p id="debug-empty" class="empty-search" hidden>No views match this search.</p>`;
    button.setAttribute('aria-expanded', 'true');
  }

  private renderDiagnostics(): void {
    const menu = this.element<HTMLElement>('#diagnostics-menu');
    const available = this.diagnostics.available && this.diagnostics.stages.length > 0;
    menu.hidden = !(this.diagnosticsOpen && available);
    if (menu.hidden) return;
    const stages = this.diagnostics.available ? this.diagnostics.stages : [];
    menu.innerHTML = `<div class="popover-heading"><div><p class="eyebrow">PROVIDER SNAPSHOTS</p><h2>Diagnostic stages</h2></div><button class="icon-button close-popover" aria-label="Close diagnostics">×</button></div>
      <label class="search-wrap"><span aria-hidden="true">⌕</span><input id="diagnostic-search" type="search" placeholder="Search ${stages.length} stages" aria-label="Search diagnostic stages"></label>
      <div class="catalog-scroll stage-list" role="listbox" aria-label="Diagnostic stages">${stages.map((stage) => `<button class="stage-option" role="option" aria-selected="${stage.id === this.activeStageId}" data-stage-id="${attr(stage.id)}" data-search="${attr(stage.label.toLowerCase())}" title="${attr(stage.label)}"><span>${escape(stage.label)}</span>${stage.id === this.activeStageId ? '<small>SELECTED</small>' : ''}</button>`).join('')}</div>
      ${this.snapshot ? `<div class="stage-snapshot"><b title="${attr(this.diagnostics.available ? stages.find(({ id }) => id === this.snapshot?.stageId)?.label ?? '' : '')}">${escape(this.diagnostics.available ? stages.find(({ id }) => id === this.snapshot?.stageId)?.label ?? 'Selected stage' : 'Selected stage')}</b><p>${escape(this.snapshot.message)}</p>${this.snapshot.values.map((item) => `<div><span>${escape(item.label)}</span><b>${escape(item.value)}</b></div>`).join('')}</div>` : '<p class="muted">Select a stage to display its provider snapshot.</p>'}
      <p id="diagnostic-empty" class="empty-search" hidden>No stages match this search.</p>`;
  }

  private renderFeatures(): void {
    const menu = this.element<HTMLElement>('#features-menu');
    const enabled = this.featuresOpen && this.featureCatalog.tables.some(({ geometry }) => geometry !== undefined);
    menu.hidden = !enabled;
    if (!enabled) return;
    const overlayTables = this.featureCatalog.tables.filter(({ geometry }) => geometry !== undefined);
    menu.innerHTML = `<div class="popover-heading"><div><p class="eyebrow">PROVIDER GEOMETRY</p><h2>Feature overlays</h2></div><button class="icon-button close-popover" aria-label="Close overlays">×</button></div>
      <div class="feature-summary">${this.featureCatalog.tables.length} tables · ${overlayTables.length} with display geometry</div>
      <div class="catalog-scroll feature-list">${this.featureCatalog.tables.map((table) => `<label class="feature-option ${table.geometry ? '' : 'no-geometry'}" title="${attr(table.label)}"><input type="checkbox" data-feature-id="${attr(table.id)}" ${this.selectedOverlays.has(table.id) ? 'checked' : ''} ${table.geometry ? '' : 'disabled'}><span>${escape(table.label)}</span><small>${formatCount(table.count)}${table.geometry ? '' : ' · no geometry'}</small></label>`).join('')}</div>`;
  }

  private toggleDebug(): void {
    if (this.catalog.views.length <= 1) return;
    this.debugOpen = !this.debugOpen;
    this.diagnosticsOpen = false;
    this.featuresOpen = false;
    this.renderDebugMenu();
    this.renderDiagnostics();
    this.renderFeatures();
    this.element<HTMLButtonElement>('#debug-button').setAttribute('aria-expanded', String(this.debugOpen));
    if (this.debugOpen) this.root.querySelector<HTMLInputElement>('#debug-search')?.focus();
  }

  private closePopovers(returnFocus: boolean): void {
    const focusSelector = this.debugOpen ? '#debug-button' : this.diagnosticsOpen ? '#diagnostics-button' : this.featuresOpen ? '#features-button' : null;
    this.debugOpen = false;
    this.diagnosticsOpen = false;
    this.featuresOpen = false;
    this.renderDebugMenu();
    this.renderDiagnostics();
    this.renderFeatures();
    this.element<HTMLButtonElement>('#debug-button').setAttribute('aria-expanded', 'false');
    this.element<HTMLButtonElement>('#diagnostics-button').setAttribute('aria-expanded', 'false');
    this.element<HTMLButtonElement>('#features-button').setAttribute('aria-expanded', 'false');
    if (returnFocus && focusSelector) this.root.querySelector<HTMLButtonElement>(focusSelector)?.focus();
  }

  private focusStage(stageId: string): void {
    const stage = [...this.root.querySelectorAll<HTMLButtonElement>('[data-stage-id]')].find((button) => button.dataset.stageId === stageId);
    stage?.focus();
  }

  private handleClick = (event: Event): void => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest('#debug-button')) { this.toggleDebug(); return; }
    if (target.closest('#diagnostics-button')) {
      this.diagnosticsOpen = !this.diagnosticsOpen;
      this.debugOpen = false;
      this.featuresOpen = false;
      this.renderDebugMenu(); this.renderFeatures(); this.renderDiagnostics();
      this.element<HTMLButtonElement>('#diagnostics-button').setAttribute('aria-expanded', String(this.diagnosticsOpen));
      if (this.diagnosticsOpen) this.root.querySelector<HTMLInputElement>('#diagnostic-search')?.focus();
      return;
    }
    if (target.closest('#features-button')) {
      this.featuresOpen = !this.featuresOpen;
      this.debugOpen = false;
      this.diagnosticsOpen = false;
      this.renderDebugMenu(); this.renderDiagnostics(); this.renderFeatures();
      this.element<HTMLButtonElement>('#features-button').setAttribute('aria-expanded', String(this.featuresOpen));
      return;
    }
    const viewButton = target.closest<HTMLElement>('[data-view-id]');
    if (viewButton?.dataset.viewId) { void this.selectView(viewButton.dataset.viewId); return; }
    const stageButton = target.closest<HTMLElement>('[data-stage-id]');
    if (stageButton?.dataset.stageId) { void this.selectStage(stageButton.dataset.stageId); return; }
    if (target.closest('.close-popover')) { this.closePopovers(true); return; }
    if (target.closest('.close-inspection')) { this.selectedPosition = null; this.pointReport = null; this.renderInspection(); this.element<HTMLButtonElement>('#inspect-center').focus(); return; }
    if (target.closest('#inspect-center')) { this.viewport.inspectCenter(); return; }
    if (target.closest('#retry-button')) { void this.openFixture(this.fixtureId); return; }
    if (target.closest('.copy-position') && this.pointReport) { void this.copy(this.pointReport.positionValue); return; }
    const copyValue = target.closest<HTMLElement>('.copy-value');
    if (copyValue) { void this.copy(`${copyValue.dataset.fieldLabel ?? ''}: ${copyValue.dataset.value ?? ''}`); return; }
    if (target.closest('#settings-button')) { this.root.classList.toggle('compact-labels'); }
  };

  private handleChange = (event: Event): void => {
    const target = event.target;
    if (!(target instanceof HTMLSelectElement || target instanceof HTMLInputElement)) return;
    if (target.id === 'fixture-selector') void this.openFixture(target.value);
    if (target.id === 'domain-selector') void this.changeDomain(target.value);
    if (target.id === 'time-selector') void this.selectTime(target.value);
    if (target instanceof HTMLInputElement && target.matches('[data-feature-id]')) {
      const id = target.dataset.featureId;
      if (id) {
        if (target.checked) this.selectedOverlays.add(id); else this.selectedOverlays.delete(id);
        this.viewport.setOverlays(this.featureCatalog.tables.filter((table) => this.selectedOverlays.has(table.id)));
      }
    }
  };

  private handleInput = (event: Event): void => {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) return;
    if (target.id === 'debug-search') this.filterOptions(target.value, '#view-catalog-list', '#debug-empty');
    if (target.id === 'diagnostic-search') this.filterOptions(target.value, '.stage-list', '#diagnostic-empty');
    if (target.id === 'field-search') this.filterFields(target.value);
  };

  private handleKeydown = (event: KeyboardEvent): void => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    if (event.key === 'Escape') {
      if (this.debugOpen || this.diagnosticsOpen || this.featuresOpen) {
        event.preventDefault(); this.closePopovers(true); return;
      }
      if (this.selectedPosition) { this.selectedPosition = null; this.pointReport = null; this.renderInspection(); this.element<HTMLButtonElement>('#inspect-center').focus(); return; }
    }
    const isDebugSearch = target.id === 'debug-search';
    const isStageSearch = target.id === 'diagnostic-search';
    const option = target.closest<HTMLButtonElement>('.view-option, .stage-option');
    const list = isDebugSearch || option?.classList.contains('view-option') ? '#view-catalog-list' : isStageSearch || option?.classList.contains('stage-option') ? '.stage-list' : null;
    if (!list) return;
    if (option && event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      const search = this.root.querySelector<HTMLInputElement>(isStageSearch || option.classList.contains('stage-option') ? '#diagnostic-search' : '#debug-search');
      if (search) { event.preventDefault(); search.focus(); search.value += event.key; this.filterOptions(search.value, isStageSearch || option.classList.contains('stage-option') ? '.stage-list' : '#view-catalog-list', isStageSearch || option.classList.contains('stage-option') ? '#diagnostic-empty' : '#debug-empty'); }
      return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Enter'].includes(event.key)) return;
    event.preventDefault();
    const isStage = list === '.stage-list';
    const selector = isStage ? '.stage-option:not([hidden])' : '.view-option:not([hidden])';
    const options = [...this.root.querySelectorAll<HTMLButtonElement>(`${list} ${selector}`)];
    if (event.key === 'Enter') { (option ?? options[0])?.click(); return; }
    if (!option) { options[0]?.focus(); return; }
    const currentGroup = option.closest<HTMLDetailsElement>('.catalog-group');
    if (currentGroup) { currentGroup.hidden = false; currentGroup.open = true; }
    const current = options.indexOf(option);
    const next = event.key === 'ArrowDown' ? Math.min(options.length - 1, current + 1) : Math.max(0, current - 1);
    const nextOption = options[next];
    const nextGroup = nextOption?.closest<HTMLDetailsElement>('.catalog-group');
    if (nextGroup) { nextGroup.hidden = false; nextGroup.open = true; }
    nextOption?.focus();
    nextOption?.scrollIntoView({ block: 'nearest' });
  };

  private filterOptions(value: string, listSelector: string, emptySelector: string): void {
    const query = value.trim().toLowerCase();
    const list = this.root.querySelector<HTMLElement>(listSelector);
    if (!list) return;
    let visible = 0;
    const buttons = [...list.querySelectorAll<HTMLButtonElement>('[data-search]')];
    for (const button of buttons) {
      const match = button.dataset.search?.includes(query) ?? true;
      button.hidden = !match;
      if (match) visible += 1;
    }
    for (const group of list.querySelectorAll<HTMLElement>('.catalog-group')) {
      const groupButtons = group.querySelectorAll<HTMLButtonElement>('[data-search]');
      const groupVisible = [...groupButtons].some((button) => !button.hidden);
      group.hidden = !groupVisible;
      if (query && groupVisible) (group as HTMLDetailsElement).open = true;
    }
    const empty = this.root.querySelector<HTMLElement>(emptySelector);
    if (empty) empty.hidden = visible !== 0;
  }

  private filterFields(value: string): void {
    const query = value.trim().toLowerCase();
    let visible = 0;
    for (const row of this.root.querySelectorAll<HTMLElement>('.field-row')) {
      row.hidden = !(row.dataset.search ?? '').includes(query);
      if (!row.hidden) visible += 1;
    }
    for (const group of this.root.querySelectorAll<HTMLDetailsElement>('.field-group')) {
      const matches = [...group.querySelectorAll<HTMLElement>('.field-row')].some((row) => !row.hidden);
      group.hidden = !matches;
      if (query && matches) group.open = true;
    }
    const groups = this.root.querySelector<HTMLElement>('#field-groups');
    let empty = this.root.querySelector<HTMLElement>('#field-empty');
    if (!empty && groups) { empty = document.createElement('p'); empty.id = 'field-empty'; empty.className = 'muted'; groups.append(empty); }
    if (empty) { empty.hidden = visible !== 0; empty.textContent = 'No fields match this search.'; }
  }

  private defaultTime(view: ViewDescriptor | null): string | undefined {
    return view?.defaultTimeSelectionId ?? view?.timeSelections?.[0]?.id;
  }

  private setFixtureQuery(fixtureId: string): void {
    const url = new URL(window.location.href);
    url.searchParams.set('fixture', fixtureId);
    window.history.replaceState({}, '', url);
  }

  private currentFixture(): FixtureOption | undefined { return this.fixtures.find(({ id }) => id === this.fixtureId); }

  private element<T extends HTMLElement>(selector: string): T {
    const result = this.root.querySelector<T>(selector);
    if (!result) throw new Error(`Missing Inspector element: ${selector}`);
    return result;
  }
}

function profileSvg(values: readonly number[]): string {
  if (values.length < 2) return '<p class="muted">No profile values supplied.</p>';
  const minimum = Math.min(...values);
  const maximum = Math.max(...values);
  const span = maximum - minimum || 1;
  const points = values.map((value, index) => `${(index / (values.length - 1) * 100).toFixed(2)},${(38 - (value - minimum) / span * 32).toFixed(2)}`).join(' ');
  return `<svg class="profile-svg" viewBox="0 0 100 42" role="img" aria-label="Provider supplied radial profile with ${values.length} samples" preserveAspectRatio="none"><polyline points="${points}" fill="none" stroke="#c4c9cd" stroke-width="1.3" vector-effect="non-scaling-stroke"/><line x1="0" y1="39" x2="100" y2="39" stroke="#555d63" stroke-width="0.5" vector-effect="non-scaling-stroke"/></svg><div class="profile-axis"><span>Center</span><span>Outer sample</span></div>`;
}

function formatNumber(value: number): string {
  if (value !== 0 && (Math.abs(value) >= 1e7 || Math.abs(value) < 1e-4)) return value.toExponential(3);
  return new Intl.NumberFormat('en-US', { maximumSignificantDigits: 5 }).format(value);
}

function formatCount(value: number): string { return new Intl.NumberFormat('en-US', { maximumSignificantDigits: 6 }).format(value); }
function formatPercent(value: number): string { return `${new Intl.NumberFormat('en-US', { maximumFractionDigits: 4 }).format(value)}%`; }
function shorten(value: string): string { return value.length <= 12 ? value : `…${value.slice(-10)}`; }
function errorTitle(failure: ProviderFailure): string {
  if (failure.category === 'missing-content') return 'Required content missing';
  if (failure.category === 'validation') return 'Provider validation failure';
  if (failure.category === 'unsupported') return 'Unsupported required feature';
  return failure.retryable ? 'Temporary load error' : 'Provider load error';
}
function escape(value: string): string { return value.replace(/[&<>"']/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character] ?? character); }
function attr(value: string): string { return escape(value); }
