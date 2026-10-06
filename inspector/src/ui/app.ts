import type {
  BodyCatalog,
  BodyProvider,
  BodySummary,
  FeatureOverlay,
  PickPosition,
  PointReport,
  ViewDescriptor,
  ViewStats,
} from '../domain';
import { Viewport } from '../render/viewport';
import { groupViews } from './view-model';

export class InspectorApp {
  private readonly viewport: Viewport;
  private provider: BodyProvider | null = null;
  private summary: BodySummary | null = null;
  private views: readonly ViewDescriptor[] = [];
  private features: readonly FeatureOverlay[] = [];
  private activeView: ViewDescriptor | null = null;
  private featuresEnabled = false;
  private autoRotate = false;

  constructor(
    private readonly root: HTMLDivElement,
    private readonly catalog: BodyCatalog,
  ) {
    this.root.innerHTML = shellMarkup();
    const viewportElement = requiredElement<HTMLDivElement>(this.root, '#body-viewport');
    this.viewport = new Viewport(viewportElement, (position) => void this.selectPoint(position));
    this.bindControls();
  }

  async start(): Promise<void> {
    try {
      const options = await this.catalog.bodies();
      const selector = requiredElement<HTMLSelectElement>(this.root, '#body-selector');
      selector.innerHTML = options.map((option) =>
        `<option value="${escapeAttribute(option.objectId)}">${escapeHtml(option.label)} · ${escapeHtml(option.subtitle.split(' · ')[0] ?? '')}</option>`,
      ).join('');
      await this.openBody(options[0]?.objectId ?? '');
    } catch (error) {
      this.showError(error);
    }
  }

  private bindControls(): void {
    requiredElement<HTMLSelectElement>(this.root, '#body-selector').addEventListener('change', (event) => {
      const target = event.currentTarget;
      if (target instanceof HTMLSelectElement) void this.openBody(target.value);
    });
    requiredElement<HTMLButtonElement>(this.root, '#debug-button').addEventListener('click', () => this.toggleDebugMenu());
    requiredElement<HTMLButtonElement>(this.root, '#features-button').addEventListener('click', () => this.toggleFeatures());
    requiredElement<HTMLButtonElement>(this.root, '#settings-button').addEventListener('click', () => this.toggleAutoRotate());
    this.root.addEventListener('click', (event) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const viewButton = target.closest<HTMLButtonElement>('[data-view-id]');
      if (viewButton?.dataset.viewId) void this.selectView(viewButton.dataset.viewId);
      if (!target.closest('#debug-menu') && !target.closest('#debug-button')) this.closeDebugMenu();
      if (target.closest('[data-dismiss-selection]')) this.hideSelection();
    });
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        this.closeDebugMenu();
        this.hideSelection();
      }
    });
  }

  private async openBody(objectId: string): Promise<void> {
    if (!objectId) return;
    this.closeDebugMenu();
    this.hideSelection();
    this.featuresEnabled = false;
    this.summary = null;
    this.provider = await this.catalog.open(objectId);
    const [summary, views, geometry, featureList, diagnostics] = await Promise.all([
      this.provider.summary(),
      this.provider.views(),
      this.provider.domainGeometry(),
      this.provider.features(),
      this.provider.diagnostics(),
    ]);
    this.summary = summary;
    this.root.classList.toggle('is-radial', summary.presentation === 'radial-profile');
    this.views = views;
    this.features = featureList;
    this.activeView = views.find((candidate) => candidate.id === summary.defaultViewId) ?? views[0] ?? null;
    requiredElement<HTMLElement>(this.root, '#body-name').textContent = summary.name;
    requiredElement<HTMLElement>(this.root, '#body-classification').textContent = summary.classification;
    requiredElement<HTMLElement>(this.root, '#body-description').textContent = summary.description;
    requiredElement<HTMLElement>(this.root, '#status-body').textContent = summary.name;
    requiredElement<HTMLElement>(this.root, '#status-object-id').textContent = `${summary.objectId.slice(0, 11)}…`;
    requiredElement<HTMLElement>(this.root, '#status-lod').textContent = summary.lod;
    requiredElement<HTMLElement>(this.root, '#status-source').textContent = summary.sourceLabel;
    requiredElement<HTMLElement>(this.root, '#domain-label').textContent = summary.domainLabel;
    requiredElement<HTMLElement>(this.root, '#viewport-instruction').textContent = summary.presentation === 'radial-profile'
      ? 'SCROLL TO ZOOM  ·  CLICK TO INSPECT PROFILE'
      : 'DRAG TO ORBIT  ·  SCROLL TO ZOOM';
    const diagnosticsBadge = requiredElement<HTMLElement>(this.root, '#diagnostics-state');
    diagnosticsBadge.textContent = diagnostics.available ? diagnostics.label ?? 'Diagnostics available' : 'Diagnostics unavailable';
    diagnosticsBadge.classList.toggle('is-available', diagnostics.available);
    const featureButton = requiredElement<HTMLButtonElement>(this.root, '#features-button');
    featureButton.hidden = featureList.length === 0;
    featureButton.setAttribute('aria-pressed', 'false');
    requiredElement<HTMLElement>(this.root, '#body-marker').textContent = summary.marker;
    await this.renderMenu();
    if (this.activeView) await this.applyView(this.activeView, geometry);
  }

  private async renderMenu(): Promise<void> {
    const menu = requiredElement<HTMLDivElement>(this.root, '#debug-menu');
    const groups = groupViews(this.views);
    const groupMarkup = Array.from(groups, ([groupName, groupViews]) => `
      <section class="debug-group" aria-label="${escapeAttribute(groupName)}">
        <div class="debug-group-title">${escapeHtml(groupName)}</div>
        ${groupViews.map((descriptor) => `
          <button class="debug-option${descriptor.id === this.activeView?.id ? ' is-active' : ''}" type="button" data-view-id="${escapeAttribute(descriptor.id)}">
            <span>${escapeHtml(descriptor.label)}</span>
            <span class="debug-option-domain">${escapeHtml(descriptor.domain)}</span>
          </button>`).join('')}
      </section>`).join('');
    menu.innerHTML = groupMarkup || '<div class="menu-empty">No declared views</div>';
  }

  private async selectView(viewId: string): Promise<void> {
    const descriptor = this.views.find((candidate) => candidate.id === viewId);
    if (!descriptor || !this.provider || !this.summary) return;
    this.activeView = descriptor;
    await this.renderMenu();
    const geometry = await this.provider.domainGeometry();
    await this.applyView(descriptor, geometry);
    this.closeDebugMenu();
  }

  private async applyView(descriptor: ViewDescriptor, geometry: Awaited<ReturnType<BodyProvider['domainGeometry']>>): Promise<void> {
    if (!this.provider || !this.summary) return;
    const [stats, tile, profile] = await Promise.all([
      this.provider.stats(descriptor.id),
      this.provider.tile(descriptor.id),
      this.provider.radialProfile(descriptor.id),
    ]);
    this.updateInfo(descriptor, stats, profile);
    this.viewport.setBody(this.summary, geometry, descriptor, tile);
    this.viewport.setFeatures(this.featuresEnabled ? this.features : []);
    this.viewport.setAutoRotate(this.autoRotate && this.summary.presentation === 'surface');
  }

  private updateInfo(descriptor: ViewDescriptor, stats: ViewStats, profile: Awaited<ReturnType<BodyProvider['radialProfile']>>): void {
    requiredElement<HTMLElement>(this.root, '#view-name').textContent = descriptor.label;
    requiredElement<HTMLElement>(this.root, '#view-domain').textContent = descriptor.domain;
    requiredElement<HTMLElement>(this.root, '#view-description').textContent = descriptor.description;
    requiredElement<HTMLElement>(this.root, '#stat-min').textContent = formatNumber(stats.min);
    requiredElement<HTMLElement>(this.root, '#stat-max').textContent = formatNumber(stats.max);
    requiredElement<HTMLElement>(this.root, '#stat-mean').textContent = formatNumber(stats.mean);
    const unit = descriptor.legend.unit ? ` ${descriptor.legend.unit}` : '';
    requiredElement<HTMLElement>(this.root, '#stats-unit').textContent = unit;
    requiredElement<HTMLElement>(this.root, '#sample-count').textContent = stats.countLabel;
    const legend = requiredElement<HTMLDivElement>(this.root, '#legend');
    const gradient = descriptor.legend.stops.map((stop) => `${stop.color} ${Math.round(stop.at * 100)}%`).join(', ');
    legend.innerHTML = `
      <div class="legend-scale" style="--legend-gradient: linear-gradient(90deg, ${escapeAttribute(gradient)})"></div>
      <div class="legend-range"><span>${formatNumber(stats.min)}${escapeHtml(unit)}</span><span>${formatNumber(stats.max)}${escapeHtml(unit)}</span></div>`;
    const radialCard = requiredElement<HTMLElement>(this.root, '#radial-profile');
    const radial = this.summary?.presentation === 'radial-profile';
    radialCard.hidden = !radial;
    if (radial) radialCard.innerHTML = radialChartMarkup(profile);
  }

  private async selectPoint(position: PickPosition): Promise<void> {
    if (!this.provider || !this.activeView) return;
    const [report, explanation] = await Promise.all([
      this.provider.inspect(position),
      this.provider.explain(position, this.activeView.id),
    ]);
    this.showSelection(report, explanation);
  }

  private showSelection(report: PointReport, explanation: readonly string[]): void {
    const card = requiredElement<HTMLElement>(this.root, '#selection-card');
    card.hidden = false;
    requiredElement<HTMLElement>(card, '#selection-position').textContent = report.positionLabel;
    requiredElement<HTMLElement>(card, '#selection-source').textContent = report.source;
    requiredElement<HTMLElement>(card, '#selection-level').textContent = report.levelUsed;
    const fieldContainer = requiredElement<HTMLDivElement>(card, '#selection-fields');
    fieldContainer.replaceChildren(...report.fields.map((field) => {
      const item = document.createElement('div');
      item.className = 'point-field';
      const label = document.createElement('span');
      label.className = 'point-field-label';
      label.textContent = field.label;
      const value = document.createElement('strong');
      value.textContent = `${field.value}${field.unit ? ` ${field.unit}` : ''}`;
      if (field.color) value.style.setProperty('--field-color', field.color);
      item.append(label, value);
      return item;
    }));
    const explanationNode = requiredElement<HTMLUListElement>(card, '#selection-explanation');
    explanationNode.replaceChildren(...explanation.map((line) => {
      const item = document.createElement('li');
      item.textContent = line;
      return item;
    }));
  }

  private hideSelection(): void {
    const card = this.root.querySelector<HTMLElement>('#selection-card');
    if (card) card.hidden = true;
  }

  private toggleDebugMenu(): void {
    const menu = requiredElement<HTMLElement>(this.root, '#debug-menu');
    const isOpen = menu.classList.toggle('is-open');
    requiredElement<HTMLButtonElement>(this.root, '#debug-button').setAttribute('aria-expanded', `${isOpen}`);
  }

  private closeDebugMenu(): void {
    this.root.querySelector<HTMLElement>('#debug-menu')?.classList.remove('is-open');
    this.root.querySelector<HTMLButtonElement>('#debug-button')?.setAttribute('aria-expanded', 'false');
  }

  private toggleFeatures(): void {
    if (this.features.length === 0) return;
    this.featuresEnabled = !this.featuresEnabled;
    const button = requiredElement<HTMLButtonElement>(this.root, '#features-button');
    button.setAttribute('aria-pressed', `${this.featuresEnabled}`);
    button.classList.toggle('is-active', this.featuresEnabled);
    this.viewport.setFeatures(this.featuresEnabled ? this.features : []);
  }

  private toggleAutoRotate(): void {
    this.autoRotate = !this.autoRotate;
    const button = requiredElement<HTMLButtonElement>(this.root, '#settings-button');
    button.setAttribute('aria-pressed', `${this.autoRotate}`);
    button.title = this.autoRotate ? 'Auto rotation on' : 'Auto rotation off';
    this.viewport.setAutoRotate(this.autoRotate && this.summary?.presentation === 'surface');
  }

  private showError(error: unknown): void {
    const message = error instanceof Error ? error.message : 'Unable to load this mock body.';
    const alert = requiredElement<HTMLElement>(this.root, '#error-message');
    alert.textContent = message;
    alert.hidden = false;
  }

  destroy(): void {
    this.viewport.destroy();
  }
}

function shellMarkup(): string {
  return `
    <main class="inspector-shell">
      <div id="body-viewport" class="body-viewport" aria-label="Interactive body viewport"></div>
      <div class="vignette" aria-hidden="true"></div>
      <header class="topbar">
        <div class="brand-lockup" aria-label="VEYRA Inspector">
          <span class="brand-mark"><i></i><b></b></span>
          <span class="brand-name">VEYRA<span class="brand-divider">/</span><small>INSPECTOR</small></span>
        </div>
        <span class="topbar-separator"></span>
        <div class="body-picker-wrap">
          <span id="body-marker" class="body-marker">◉</span>
          <select id="body-selector" aria-label="Select body" data-testid="body-selector"></select>
          <span class="select-chevron">⌄</span>
        </div>
        <div class="toolbar-actions">
          <div class="debug-wrap">
            <button id="debug-button" class="toolbar-button debug-button" type="button" aria-expanded="false" aria-haspopup="true" data-testid="debug-button">
              <span class="cube-icon" aria-hidden="true"><i></i><b></b><em></em></span>
              <span>Debug</span><span class="button-caret">⌄</span>
            </button>
            <div id="debug-menu" class="debug-menu" role="menu" data-testid="debug-menu"></div>
          </div>
          <button id="features-button" class="toolbar-button feature-button" type="button" aria-pressed="false" hidden>
            <span class="feature-symbol" aria-hidden="true">⌁</span><span>Features</span>
          </button>
          <span class="toolbar-spacer"></span>
          <button id="settings-button" class="icon-button" type="button" title="Auto rotation off" aria-label="Toggle auto rotation" aria-pressed="false">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 8.1a3.9 3.9 0 1 0 0 7.8 3.9 3.9 0 0 0 0-7.8Z"/><path d="m19.3 13.6 1.1.9-1.4 2.4-1.4-.5a7.3 7.3 0 0 1-1.4.8l-.2 1.5h-2.8l-.3-1.5a7.3 7.3 0 0 1-1.4-.8l-1.4.5-1.4-2.4 1.1-.9a7.2 7.2 0 0 1 0-1.6l-1.1-.9 1.4-2.4 1.4.5a7.3 7.3 0 0 1 1.4-.8l.3-1.5h2.8l.2 1.5a7.3 7.3 0 0 1 1.4.8l1.4-.5 1.4 2.4-1.1.9a7.2 7.2 0 0 1 0 1.6Z"/></svg>
          </button>
        </div>
      </header>

      <section class="body-heading" aria-live="polite">
        <div class="eyebrow"><span class="live-dot"></span>BODY ARTIFACT <span class="heading-dot">/</span> <span id="domain-label">SURFACE DOMAIN</span></div>
        <div id="body-name" class="body-heading-name">Veyra</div>
        <div id="body-classification" class="body-heading-class">Terrestrial · habitable test body</div>
        <p id="body-description" class="body-heading-description">A broad, quiet view of an ocean-bearing rocky world.</p>
      </section>

      <aside class="view-card glass-card" aria-live="polite">
        <div class="card-kicker"><span class="kicker-line"></span>ACTIVE VIEW <span id="view-domain" class="view-domain">surface</span></div>
        <h1 id="view-name">Surface form</h1>
        <p id="view-description">A presentation map of the mock body surface.</p>
        <div id="legend" class="legend"></div>
        <div class="stats-heading"><span>VIEW STATISTICS</span><span id="stats-unit"></span></div>
        <div class="stats-grid">
          <div><span>MIN</span><strong id="stat-min">−0.8</strong></div>
          <div><span>MEAN</span><strong id="stat-mean">0.18</strong></div>
          <div><span>MAX</span><strong id="stat-max">1.0</strong></div>
        </div>
        <div id="sample-count" class="sample-count">12.4M samples</div>
        <div id="radial-profile" class="radial-profile" hidden></div>
      </aside>

      <section id="selection-card" class="selection-card glass-card" hidden aria-live="polite" data-testid="selection-card">
        <div class="selection-head">
          <div><div class="card-kicker"><span class="kicker-line"></span>POINT INSPECTOR</div><h2 id="selection-position">Surface 0.00, 0.00, 1.00</h2></div>
          <button class="close-selection" type="button" aria-label="Close point inspector" data-dismiss-selection>×</button>
        </div>
        <div id="selection-fields" class="selection-fields"></div>
        <div class="selection-meta"><span id="selection-source">Mock provider</span><span id="selection-level">L09 · native</span></div>
        <details class="explain-details"><summary>Value context</summary><ul id="selection-explanation"></ul></details>
      </section>

      <div id="error-message" class="error-card" role="alert" hidden></div>
      <div class="viewport-hint"><span class="mouse-hint" aria-hidden="true">◌</span><span id="viewport-instruction">DRAG TO ORBIT  ·  SCROLL TO ZOOM</span></div>
      <footer class="status-strip" aria-label="Body status">
        <span class="status-item"><i class="status-led"></i><b id="status-body">Veyra</b></span>
        <span class="status-divider"></span>
        <span class="status-item status-muted">OBJECT <b id="status-object-id">obj:9f3c2a…</b></span>
        <span class="status-divider"></span>
        <span class="status-item status-muted">LOD <b id="status-lod">L09 · native</b></span>
        <span class="status-divider"></span>
        <span class="status-item status-muted status-source"><span class="source-mark"></span><b id="status-source">Mock provider</b></span>
        <span class="status-right"><span id="diagnostics-state">Diagnostics unavailable</span><span class="status-divider"></span>DISPLAY ONLY</span>
      </footer>
    </main>`;
}

function radialChartMarkup(profile: Awaited<ReturnType<BodyProvider['radialProfile']>>): string {
  const points = profile.map(({ radius, value }) => `${(radius * 230 + 8).toFixed(1)},${(43 - value * 33).toFixed(1)}`).join(' ');
  return `
    <div class="profile-heading"><span>RADIAL PROFILE</span><span>CORE → PHOTOSPHERE</span></div>
    <svg class="profile-chart" viewBox="0 0 246 54" preserveAspectRatio="none" role="img" aria-label="Radial profile line chart">
      <defs><linearGradient id="profile-fill" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="#e9a87f" stop-opacity=".22"/><stop offset="1" stop-color="#e9a87f" stop-opacity="0"/></linearGradient></defs>
      <path class="profile-area" d="M 8,48 L ${points.replaceAll(' ', ' L ')} L 238,48 Z" />
      <polyline points="${points}" />
    </svg>
    <div class="profile-axis"><span>0</span><span>0.5 R★</span><span>1.0 R★</span></div>`;
}

function requiredElement<T extends Element>(root: ParentNode, selector: string): T {
  const element = root.querySelector<T>(selector);
  if (!element) throw new Error(`Inspector interface is missing ${selector}`);
  return element;
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat('en-US', { maximumFractionDigits: 2 }).format(value);
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character] ?? character);
}

function escapeAttribute(value: string): string {
  return escapeHtml(value);
}
