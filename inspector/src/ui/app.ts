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
import { normalizeCssColor, normalizePaletteStops } from '../render/css-color';
import { Viewport } from '../render/viewport';
import { orderedGroups } from './view-model';
import { buildRadialProfilePlot } from './profile-plot';

interface ExplainContext {
  provider: BodyProvider;
  fixtureId: string;
  domainId: string;
  positionKey: string;
  position: PickPosition;
  viewId: string;
}

interface ExplainSnapshot {
  context: ExplainContext;
  chain: readonly ExplainStep[];
}

interface ExplainFailure {
  context: ExplainContext;
  failure: ProviderFailure;
}

interface FixtureLoadProgress {
  fixtureId: string;
  provider: BodyProvider | null;
  summary: BodySummary | null;
  domains: readonly DomainDescriptor[] | null;
  diagnostics: DiagnosticsDescriptor | null;
  featureCatalog: FeatureCatalog | null;
  domainId: string | null;
  catalog: ViewCatalog | null;
  geometry: RenderGeometry | null;
}

type FixtureLoadOperation = 'open' | 'summary' | 'domains' | 'diagnostics' | 'features' | 'views' | 'geometry';

type RetryIntent =
  | { kind: 'fixture-load'; progress: FixtureLoadProgress; operation: FixtureLoadOperation }
  | { kind: 'domain-load'; fixtureId: string; provider: BodyProvider; domainId: string }
  | { kind: 'view-load'; fixtureId: string; provider: BodyProvider; domainId: string; viewId: string; timeId: string | undefined; stageId: string | undefined }
  | { kind: 'diagnostic-stage'; fixtureId: string; provider: BodyProvider; domainId: string | null; stageId: string }
  | { kind: 'inspection'; fixtureId: string; provider: BodyProvider; domainId: string | null; position: PickPosition }
  | { kind: 'explanation'; context: ExplainContext };

function sameExplainContext(left: ExplainContext, right: ExplainContext): boolean {
  return left.provider === right.provider
    && left.fixtureId === right.fixtureId
    && left.domainId === right.domainId
    && left.positionKey === right.positionKey
    && left.viewId === right.viewId;
}

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
  private inspectionPending = false;
  private failure: ProviderFailure | null = null;
  private failureOwner: RetryIntent['kind'] | null = null;
  private retryIntent: RetryIntent | null = null;
  private loadState: LoadState = 'opening';
  private loadStateBeforeFailure: LoadState | null = null;
  private debugOpen = false;
  private diagnosticsOpen = false;
  private featuresOpen = false;
  private requestVersion = 0;
  private inspectionRequestVersion = 0;
  private explainRequestVersion = 0;
  private snapshotExplain: ExplainSnapshot | null = null;
  private pendingExplain: ExplainContext | null = null;
  private explainFailure: ExplainFailure | null = null;
  private destroyed = false;

  constructor(private readonly root: HTMLDivElement, private readonly bodyCatalog: BodyCatalog) {
    this.fixtures = bodyCatalog.fixtures();
    const requested = new URL(window.location.href).searchParams.get('fixture');
    this.fixtureId = this.fixtures.find(({ id }) => id === requested)?.id ?? this.fixtures[0]?.id ?? '';
    this.root.innerHTML = this.shell();
    this.viewport = new Viewport(this.element<HTMLDivElement>('#viewport'), (position) => void this.inspect(position));
    this.root.addEventListener('click', this.handleClick);
    this.root.addEventListener('change', this.handleChange);
    this.root.addEventListener('input', this.handleInput);
    this.root.addEventListener('keydown', this.handleKeydown);
  }

  async start(): Promise<void> {
    if (this.destroyed) return;
    if (this.fixtures.length === 0) {
      this.loadState = 'ready';
      this.renderAll();
      return;
    }
    this.renderAll();
    await this.openFixture(this.fixtureId);
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.requestVersion += 1;
    this.inspectionRequestVersion += 1;
    this.explainRequestVersion += 1;
    this.pendingExplain = null;
    this.root.removeEventListener('click', this.handleClick);
    this.root.removeEventListener('change', this.handleChange);
    this.root.removeEventListener('input', this.handleInput);
    this.root.removeEventListener('keydown', this.handleKeydown);
    this.viewport.destroy();
  }

  private isCurrentRequest(version: number): boolean {
    return !this.destroyed && version === this.requestVersion;
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
          <button id="inspect-center" class="toolbar-button inspect-action" title="Inspect the center of the displayed model" disabled>Inspect point</button>
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
    this.inspectionRequestVersion += 1;
    this.invalidateExplain();
    this.clearFailure();
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
    this.inspectionPending = false;
    this.loadState = 'opening';
    this.setFixtureQuery(fixtureId);
    this.closePopovers(false);
    this.viewport.clearData();
    this.renderAll();
    const progress: FixtureLoadProgress = {
      fixtureId,
      provider: null,
      summary: null,
      domains: null,
      diagnostics: null,
      featureCatalog: null,
      domainId: null,
      catalog: null,
      geometry: null,
    };
    await this.continueFixtureLoad(progress, version);
  }

  private async continueFixtureLoad(
    progress: FixtureLoadProgress,
    version: number,
    retryOperation?: FixtureLoadOperation,
  ): Promise<void> {
    let provider = progress.provider;
    if (!provider) {
      try {
        provider = await this.bodyCatalog.open(progress.fixtureId);
      } catch (error) {
        if (this.isCurrentRequest(version)) this.showFailure(error, { kind: 'fixture-load', progress, operation: 'open' });
        return;
      }
      if (!this.isCurrentRequest(version)) return;
      progress.provider = provider;
      this.provider = provider;
    }

    if (!this.isCurrentRequest(version)) return;
    this.loadState = 'metadata';
    this.renderStatus();

    if (retryOperation && ['summary', 'domains', 'diagnostics', 'features'].includes(retryOperation)) {
      try {
        switch (retryOperation) {
          case 'summary': {
            const value = await provider.summary();
            if (!this.isCurrentRequest(version)) return;
            progress.summary = value;
            break;
          }
          case 'domains': {
            const value = await provider.domains();
            if (!this.isCurrentRequest(version)) return;
            progress.domains = value;
            break;
          }
          case 'diagnostics': {
            const value = await provider.diagnostics();
            if (!this.isCurrentRequest(version)) return;
            progress.diagnostics = value;
            break;
          }
          case 'features': {
            const value = await provider.features();
            if (!this.isCurrentRequest(version)) return;
            progress.featureCatalog = value;
            break;
          }
        }
      } catch (error) {
        if (this.isCurrentRequest(version)) this.showFailure(error, { kind: 'fixture-load', progress, operation: retryOperation });
        return;
      }
      return this.continueFixtureLoad(progress, version);
    }

    const [summaryResult, domainsResult, diagnosticsResult, featuresResult] = await Promise.all([
      settle(progress.summary ? Promise.resolve(progress.summary) : provider.summary()),
      settle(progress.domains ? Promise.resolve(progress.domains) : provider.domains()),
      settle(progress.diagnostics ? Promise.resolve(progress.diagnostics) : provider.diagnostics()),
      settle(progress.featureCatalog ? Promise.resolve(progress.featureCatalog) : provider.features()),
    ]);
    if (!this.isCurrentRequest(version)) return;
    if (summaryResult.ok) progress.summary = summaryResult.value;
    if (domainsResult.ok) progress.domains = domainsResult.value;
    if (diagnosticsResult.ok) progress.diagnostics = diagnosticsResult.value;
    if (featuresResult.ok) progress.featureCatalog = featuresResult.value;
    const metadataFailure = !summaryResult.ok
      ? { operation: 'summary' as const, error: summaryResult.error }
      : !domainsResult.ok
        ? { operation: 'domains' as const, error: domainsResult.error }
        : !diagnosticsResult.ok
          ? { operation: 'diagnostics' as const, error: diagnosticsResult.error }
          : !featuresResult.ok
            ? { operation: 'features' as const, error: featuresResult.error }
            : null;
    if (metadataFailure) {
      this.showFailure(metadataFailure.error, { kind: 'fixture-load', progress, operation: metadataFailure.operation });
      return;
    }

    this.summary = progress.summary;
    this.domains = progress.domains!;
    this.diagnostics = progress.diagnostics!;
    this.featureCatalog = progress.featureCatalog!;
    const domain = progress.domains![0] ?? null;
    this.activeDomain = domain;
    progress.domainId = domain?.id ?? null;
    if (domain) {
      if (!progress.catalog) {
        try {
          const catalog = await provider.views(domain.id);
          if (!this.isCurrentRequest(version)) return;
          progress.catalog = catalog;
        } catch (error) {
          if (this.isCurrentRequest(version)) this.showFailure(error, { kind: 'fixture-load', progress, operation: 'views' });
          return;
        }
      }
      this.catalog = progress.catalog;
      this.activeView = this.catalog.views[0] ?? null;
      this.activeTimeId = this.defaultTime(this.activeView);
      if (!progress.geometry) {
        try {
          const geometry = await provider.domainGeometry(domain.id);
          if (!this.isCurrentRequest(version)) return;
          progress.geometry = geometry;
        } catch (error) {
          if (this.isCurrentRequest(version)) this.showFailure(error, { kind: 'fixture-load', progress, operation: 'geometry' });
          return;
        }
      }
      this.geometry = progress.geometry;
    }
    if (!this.isCurrentRequest(version)) return;
    this.loadState = 'ready';
    this.renderAll();
    if (this.activeView) await this.loadActiveView(version);
  }

  private async changeDomain(domainId: string): Promise<void> {
    const domain = this.domains.find(({ id }) => id === domainId);
    if (!domain || !this.provider) return;
    const provider = this.provider;
    const version = ++this.requestVersion;
    this.inspectionRequestVersion += 1;
    this.invalidateExplain();
    this.closePopovers(false);
    this.clearFailure();
    this.activeDomain = domain;
    this.catalog = { groups: [], views: [] };
    this.activeView = null;
    this.activeTimeId = undefined;
    this.activeStageId = undefined;
    this.snapshot = null;
    this.pointReport = null;
    this.selectedPosition = null;
    this.inspectionPending = false;
    this.geometry = null;
    this.stats = undefined;
    this.viewport.clearData();
    this.loadState = 'metadata';
    this.renderAll();
    let catalog: ViewCatalog;
    let geometry: RenderGeometry;
    try {
      [catalog, geometry] = await Promise.all([provider.views(domain.id), provider.domainGeometry(domain.id)]);
    } catch (error) {
      if (!this.isCurrentRequest(version)) return;
      this.showFailure(error, { kind: 'domain-load', fixtureId: this.fixtureId, provider, domainId: domain.id });
      return;
    }
    if (!this.isCurrentRequest(version)) return;
    this.catalog = catalog;
    this.geometry = geometry;
    this.activeView = catalog.views[0] ?? null;
    this.activeTimeId = this.defaultTime(this.activeView);
    this.loadState = 'ready';
    this.renderAll();
    if (this.activeView) await this.loadActiveView(version);
    if (!this.isCurrentRequest(version)) return;
    this.element<HTMLSelectElement>('#domain-selector').focus();
  }

  private async selectView(viewId: string): Promise<void> {
    const view = this.catalog.views.find(({ id }) => id === viewId);
    if (!view || !this.provider) return;
    const version = ++this.requestVersion;
    this.invalidateExplain();
    this.retireIncompleteInspection();
    this.clearFailure();
    this.activeView = view;
    this.activeTimeId = this.defaultTime(view);
    this.stats = undefined;
    this.closePopovers(true);
    this.loadState = 'view';
    this.viewport.clearData();
    this.renderAll();
    await this.loadActiveView(version);
  }

  private async loadActiveView(version: number): Promise<void> {
    if (this.destroyed) return;
    const provider = this.provider;
    const view = this.activeView;
    const domain = this.activeDomain;
    if (!provider || !view || !domain) {
      this.loadState = 'ready';
      this.renderAll();
      return;
    }
    const timeId = this.activeTimeId;
    const stageId = this.activeStageId;
    this.retireIncompleteInspection();
    this.clearFailure();
    this.stats = undefined;
    this.loadState = 'view';
    this.renderStatus();
    this.renderLegend();
    const statsPromise = Promise.resolve()
      .then(() => this.isCurrentRequest(version) ? provider.stats(view.id) : undefined)
      .then((value) => ({ ok: true as const, value }), () => ({ ok: false as const }));
    let geometry: RenderGeometry;
    let tile: Awaited<ReturnType<BodyProvider['tile']>>;
    try {
      [geometry, tile] = await Promise.all([
        this.geometry ? Promise.resolve(this.geometry) : provider.domainGeometry(domain.id),
        provider.tile(view.id, timeId, stageId),
      ]);
    } catch (error) {
      if (!this.isCurrentRequest(version)) return;
      this.viewport.clearData();
      this.showFailure(error, {
        kind: 'view-load',
        fixtureId: this.fixtureId,
        provider,
        domainId: domain.id,
        viewId: view.id,
        timeId,
        stageId,
      });
      return;
    }
    if (!this.isCurrentRequest(version)) return;
    this.geometry = geometry;
    this.loadState = 'ready';
    this.viewport.setData(geometry, view, tile, JSON.stringify([this.fixtureId, domain.id]));
    this.viewport.setOverlays(this.featureCatalog.tables.filter((table) => this.selectedOverlays.has(table.id)));
    const missingResources = tile.missingResources ?? [];
    this.renderAll(missingResources);
    void statsPromise.then((statsResult) => {
      if (!this.isCurrentRequest(version) || !statsResult.ok || statsResult.value === undefined) return;
      this.stats = statsResult.value;
      this.renderLegend(missingResources);
    }).catch(() => undefined);
  }

  private async inspect(position: PickPosition): Promise<void> {
    if (this.destroyed || !this.provider) return;
    const provider = this.provider;
    const fixtureId = this.fixtureId;
    const domainId = this.activeDomain?.id;
    const version = ++this.inspectionRequestVersion;
    this.invalidateExplain();
    this.clearFailure();
    this.selectedPosition = position;
    this.pointReport = null;
    this.inspectionPending = true;
    this.renderInspection();
    let report: PointReport;
    try {
      report = await provider.inspect(position);
    } catch (error) {
      if (this.isCurrentInspection(version, provider, fixtureId, domainId, position)) {
        this.inspectionPending = false;
        this.showFailure(error, { kind: 'inspection', fixtureId, provider, domainId: domainId ?? null, position });
      }
      return;
    }
    if (!this.isCurrentInspection(version, provider, fixtureId, domainId, position)) return;
    this.inspectionPending = false;
    this.pointReport = report;
    this.renderInspection();
    const input = this.root.querySelector<HTMLInputElement>('#field-search');
    if (input) input.focus();
  }

  private isCurrentInspection(
    version: number,
    provider: BodyProvider,
    fixtureId: string,
    domainId: string | undefined,
    position: PickPosition,
  ): boolean {
    return !this.destroyed
      && version === this.inspectionRequestVersion
      && this.provider === provider
      && this.fixtureId === fixtureId
      && this.activeDomain?.id === domainId
      && this.selectedPosition !== null
      && positionKey(this.selectedPosition) === positionKey(position);
  }

  private invalidateExplain(): void {
    this.explainRequestVersion += 1;
    this.snapshotExplain = null;
    this.pendingExplain = null;
    this.explainFailure = null;
  }

  private retireIncompleteInspection(): void {
    if (!this.selectedPosition || this.pointReport) return;
    this.inspectionRequestVersion += 1;
    this.selectedPosition = null;
    this.pointReport = null;
    this.inspectionPending = false;
    this.renderInspection();
  }

  private clearInspectionFailure(): void {
    if (this.failureOwner === 'inspection' || this.failureOwner === 'explanation') this.clearFailure();
  }

  private closeInspection(): void {
    this.inspectionRequestVersion += 1;
    this.invalidateExplain();
    this.clearInspectionFailure();
    this.selectedPosition = null;
    this.pointReport = null;
    this.inspectionPending = false;
    this.renderInspection();
    const inspectButton = this.element<HTMLButtonElement>('#inspect-center');
    if (inspectButton.disabled) this.element<HTMLSelectElement>('#fixture-selector').focus();
    else inspectButton.focus();
  }

  private async selectStage(stageId: string): Promise<void> {
    if (!this.provider) return;
    const provider = this.provider;
    this.activeStageId = stageId;
    this.retireIncompleteInspection();
    this.clearFailure();
    this.snapshot = null;
    const version = ++this.requestVersion;
    this.renderDiagnostics();
    this.focusStage(stageId);
    let snapshot: DiagnosticSnapshot;
    try {
      snapshot = await provider.diagnosticStage(stageId);
    } catch (error) {
      if (this.isCurrentRequest(version)) {
        this.showFailure(error, {
          kind: 'diagnostic-stage',
          fixtureId: this.fixtureId,
          provider,
          domainId: this.activeDomain?.id ?? null,
          stageId,
        });
      }
      return;
    }
    if (!this.isCurrentRequest(version)) return;
    this.snapshot = snapshot;
    this.renderDiagnostics();
    this.focusStage(stageId);
    if (this.activeView) await this.loadActiveView(version);
    if (!this.isCurrentRequest(version)) return;
    this.focusStage(stageId);
  }

  private async selectTime(timeId: string): Promise<void> {
    if (!this.activeView?.timeSelections?.some(({ id }) => id === timeId)) return;
    this.retireIncompleteInspection();
    this.clearFailure();
    this.activeTimeId = timeId;
    const version = ++this.requestVersion;
    this.loadState = 'view';
    this.viewport.clearData();
    this.renderStatus();
    await this.loadActiveView(version);
    if (!this.isCurrentRequest(version)) return;
    this.element<HTMLSelectElement>('#time-selector').focus();
  }

  private async copy(text: string): Promise<void> {
    if (this.destroyed) return;
    try {
      await navigator.clipboard.writeText(text);
      if (!this.destroyed) this.setCopyStatus('Copied');
    } catch {
      if (!this.destroyed) this.setCopyStatus('Clipboard unavailable');
    }
  }

  private setCopyStatus(message: string): void {
    const output = this.root.querySelector<HTMLElement>('#copy-status');
    if (output) output.textContent = message;
  }

  private clearFailure(): void {
    this.failure = null;
    this.failureOwner = null;
    this.retryIntent = null;
    if (this.loadState === 'error') {
      this.loadState = this.loadStateBeforeFailure ?? (this.activeView ? 'ready' : 'metadata');
      this.loadStateBeforeFailure = null;
    }
    this.renderError();
    this.renderStatus();
  }

  private showFailure(error: unknown, retryIntent?: RetryIntent): void {
    if (this.destroyed) return;
    const failure = asProviderFailure(error);
    if (this.debugOpen || this.diagnosticsOpen || this.featuresOpen) this.closePopovers(false);
    if (this.loadState !== 'error') this.loadStateBeforeFailure = this.loadState;
    this.failure = failure;
    this.failureOwner = retryIntent?.kind ?? null;
    this.retryIntent = failure.retryable && retryIntent && this.isRetryIntentCurrent(retryIntent) ? retryIntent : null;
    this.loadState = 'error';
    this.renderAll();
  }

  private isRetryIntentCurrent(intent: RetryIntent): boolean {
    switch (intent.kind) {
      case 'fixture-load':
        return this.fixtureId === intent.progress.fixtureId
          && this.provider === intent.progress.provider
          && (!['views', 'geometry'].includes(intent.operation) || this.activeDomain?.id === intent.progress.domainId);
      case 'domain-load':
        return this.fixtureId === intent.fixtureId && this.provider === intent.provider && this.activeDomain?.id === intent.domainId;
      case 'view-load':
        return this.fixtureId === intent.fixtureId
          && this.provider === intent.provider
          && this.activeDomain?.id === intent.domainId
          && this.activeView?.id === intent.viewId
          && this.activeTimeId === intent.timeId
          && this.activeStageId === intent.stageId;
      case 'diagnostic-stage':
        return this.fixtureId === intent.fixtureId
          && this.provider === intent.provider
          && (this.activeDomain?.id ?? null) === intent.domainId
          && this.activeStageId === intent.stageId;
      case 'inspection':
        return this.fixtureId === intent.fixtureId
          && this.provider === intent.provider
          && (this.activeDomain?.id ?? null) === intent.domainId
          && this.selectedPosition !== null
          && positionKey(this.selectedPosition) === positionKey(intent.position);
      case 'explanation': {
        const current = this.currentExplainContext();
        return current !== null && sameExplainContext(current, intent.context);
      }
    }
  }

  private retryFailure(): void {
    const intent = this.retryIntent;
    if (!this.failure?.retryable || !intent || !this.isRetryIntentCurrent(intent)) {
      this.clearFailure();
      return;
    }
    this.clearFailure();
    switch (intent.kind) {
      case 'fixture-load':
        {
          const version = ++this.requestVersion;
          this.loadState = intent.progress.provider ? 'metadata' : 'opening';
          void this.continueFixtureLoad(intent.progress, version, intent.operation);
        }
        return;
      case 'domain-load':
        void this.changeDomain(intent.domainId);
        return;
      case 'view-load':
        this.retryViewLoad();
        return;
      case 'diagnostic-stage':
        this.diagnosticsOpen = true;
        this.debugOpen = false;
        this.featuresOpen = false;
        this.renderDebugMenu();
        this.renderFeatures();
        this.renderDiagnostics();
        this.syncPopoverTriggers();
        void this.selectStage(intent.stageId);
        return;
      case 'inspection':
        void this.inspect(intent.position);
        return;
      case 'explanation':
        this.retryExplain(intent.context);
    }
  }

  private retryViewLoad(): void {
    const version = ++this.requestVersion;
    this.loadState = 'view';
    this.viewport.clearData();
    this.renderStatus();
    void this.loadActiveView(version);
  }

  private retryExplain(context: ExplainContext): void {
    const current = this.currentExplainContext();
    if (!current || !sameExplainContext(current, context)) return;
    this.explainRequestVersion += 1;
    this.pendingExplain = null;
    this.snapshotExplain = null;
    this.explainFailure = null;
    this.renderInspection();
  }

  private renderAll(missingResources: readonly string[] = []): void {
    if (this.destroyed) return;
    this.renderInspectCenterButton();
    this.renderFixtureSelector();
    this.renderIdentity();
    if (this.fixtures.length === 0) this.element<HTMLElement>('#body-name').textContent = 'No fixtures available';
    this.renderDomainSelector();
    this.renderDebugButton();
    this.renderTemporalControl();
    this.renderDiagnosticsButton();
    this.renderFeaturesButton();
    this.syncPopoverTriggers();
    this.renderStatus(missingResources);
    this.renderError();
    this.renderLegend(missingResources);
    this.renderInspection();
    this.renderDebugMenu();
    this.renderDiagnostics();
    this.renderFeatures();
  }

  private renderInspectCenterButton(): void {
    this.element<HTMLButtonElement>('#inspect-center').disabled = !this.viewport.hasInspectableModel;
  }

  private renderFixtureSelector(): void {
    const selector = this.element<HTMLSelectElement>('#fixture-selector');
    selector.innerHTML = this.fixtures.map((fixture) => `<option value="${attr(fixture.id)}" title="${attr(fixture.description)}">${escape(fixture.label)}</option>`).join('');
    selector.disabled = this.fixtures.length === 0;
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

  private syncPopoverTriggers(): void {
    this.renderDebugButton();
    this.renderDiagnosticsButton();
    this.renderFeaturesButton();
  }

  private renderStatus(missingResources: readonly string[] = []): void {
    this.renderInspectCenterButton();
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
      ${failure.retryable && this.retryIntent && this.isRetryIntentCurrent(this.retryIntent) ? '<button id="retry-button" class="secondary-button">Retry</button>' : ''}`;
  }

  private renderLegend(missingResources: readonly string[] = []): void {
    const panel = this.element<HTMLElement>('#legend-panel');
    const view = this.activeView;
    if (this.fixtures.length === 0) {
      panel.innerHTML = '<div class="panel-heading"><p class="eyebrow">CURRENT VIEW</p><h2 id="view-name">No fixtures available</h2></div><p class="muted">The catalog does not declare any fixtures to open.</p>';
      return;
    }
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
      ${this.activeDomain?.renderKind === 'radial-profile' && this.geometry?.profile ? profileSvg(this.geometry.profile) : ''}
      ${missingResources.length ? `<div class="incomplete-note" role="status"><b>Incomplete response</b><span>${missingResources.map((item) => `<span title="${attr(item)}">${escape(item)}</span>`).join(', ')}</span></div>` : ''}
      <p class="legend-source">All values are synthetic fixture responses.</p>`;
    panel.dataset.viewId = view.id;
    this.applyLegendColors(panel, view);
  }

  private renderLegendContent(view: ViewDescriptor, stats: ViewStats | undefined): string {
    const legend = view.legend;
    if (legend.kind === 'categorical') {
      return `<div class="legend-unit-row">${legend.unit ? `<span class="unit-tag">${escape(legend.unit)}</span>` : '<span class="muted">No unit provided</span>'}<span class="muted">${legend.categories.length} categories</span></div>
        <div class="category-list" role="list" aria-label="Legend categories">${legend.categories.map((category, index) => `<div class="category-row" role="listitem" tabindex="0" title="${attr(category.label)}"><span class="category-swatch" data-category-color-index="${index}"></span><span class="category-name" title="${attr(category.label)}">${escape(category.label)}</span><span class="category-metrics">${category.count === undefined ? '—' : formatCount(category.count)}${category.weightedPercent === undefined ? '' : `<small>${formatPercent(category.weightedPercent)}</small>`}</span></div>`).join('')}</div>`;
    }
    const statText = legend.statsAvailable && stats
      ? `<div class="stats-grid"><div><span>MIN</span><b title="${attr(String(stats.min))}">${formatNumber(stats.min)}</b></div><div><span>MAX</span><b title="${attr(String(stats.max))}">${formatNumber(stats.max)}</b></div><div><span>MEAN</span><b title="${attr(String(stats.mean))}">${formatNumber(stats.mean)}</b></div></div>${stats.count === undefined ? '' : `<p class="sample-count">${formatCount(stats.count)} provider records</p>`}`
      : '<p class="no-stats">No statistics available</p>';
    return `<div class="legend-unit-row">${legend.unit ? `<span class="unit-tag">${escape(legend.unit)}</span>` : '<span class="muted">No unit provided</span>'}${legend.range ? `<span class="muted range-value" title="${attr(`${legend.range.min} – ${legend.range.max}`)}">${formatNumber(legend.range.min)} – ${formatNumber(legend.range.max)}</span>` : ''}</div>
      <div class="legend-scale" role="img" aria-label="Continuous colour scale${legend.unit ? ` in ${attr(legend.unit)}` : ''}"></div>
      <div class="scale-ends"><span>${legend.range ? formatNumber(legend.range.min) : 'Low'}</span><span>${legend.range ? formatNumber(legend.range.max) : 'High'}</span></div>${statText}`;
  }

  private applyLegendColors(panel: HTMLElement, view: ViewDescriptor): void {
    if (view.legend.kind === 'categorical') {
      view.legend.categories.forEach((category, index) => {
        const color = normalizeCssColor(category.color);
        const swatch = panel.querySelector<HTMLElement>(`[data-category-color-index="${index}"]`);
        if (color && swatch) swatch.style.setProperty('--swatch', color.css);
      });
      return;
    }
    const stops = normalizePaletteStops(view.legend.stops)
      .map(({ at, color }) => `${color.css} ${(at * 100).toFixed(0)}%`);
    const scale = panel.querySelector<HTMLElement>('.legend-scale');
    if (stops.length > 0 && scale) scale.style.setProperty('--scale', `linear-gradient(90deg, ${stops.join(', ')})`);
  }

  private renderInspection(): void {
    const panel = this.element<HTMLElement>('#inspection-panel');
    if (!this.selectedPosition) {
      panel.hidden = true;
      panel.innerHTML = '';
      return;
    }
    panel.hidden = false;
    if (!this.pointReport && this.failureOwner === 'inspection' && this.failure) {
      panel.classList.remove('large-report');
      panel.innerHTML = `<div class="panel-heading"><div><p class="eyebrow">SELECTED POSITION</p><h2>Point inspection failed</h2></div><button class="icon-button close-inspection" aria-label="Close point inspection">×</button></div><p class="muted">${escape(this.failure.message)}</p>`;
      return;
    }
    if (!this.pointReport && !this.inspectionPending) {
      panel.hidden = true;
      panel.innerHTML = '';
      return;
    }
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
    const context = this.currentExplainContext();
    const explainSnapshot = context && this.snapshotExplain && sameExplainContext(context, this.snapshotExplain.context)
      ? this.snapshotExplain.chain
      : null;
    const explainFailure = context && this.explainFailure && sameExplainContext(context, this.explainFailure.context)
      ? this.explainFailure.failure
      : null;
    const explainContent = explainSnapshot
      ? this.renderExplain(explainSnapshot)
      : explainFailure
        ? `<p class="explain-error" role="alert">${escape(explainFailure.message)}</p>`
        : '<p class="explain-loading">Loading explanation…</p>';
    const explain = context
      ? `<details class="explain-block"><summary>Provider explanation</summary><div class="explain-tree">${explainContent}</div></details>`
      : '';
    panel.innerHTML = `<header class="inspection-header"><div><p class="eyebrow">SELECTED POSITION · ${count} FIELDS</p><h2 title="${attr(report.positionLabel)}">${escape(report.positionLabel)}</h2><code title="${attr(report.positionValue)}">${escape(report.positionValue)}</code></div><button class="icon-button close-inspection" aria-label="Close point inspection">×</button></header>
      ${count > 8 ? '<label class="search-wrap field-search-wrap"><span aria-hidden="true">⌕</span><input id="field-search" type="search" placeholder="Filter fields and groups" aria-label="Filter point fields"></label>' : ''}
      <div class="report-scroll"><div class="field-groups" id="field-groups">${groups || '<p class="muted">No fields supplied for this position.</p>'}</div>
      ${explain}<div class="copy-row"><button class="secondary-button copy-position">Copy position</button><span id="copy-status" class="sr-only" aria-live="polite"></span></div></div>`;
    if (context && !explainSnapshot && !explainFailure) void this.loadExplain(context);
  }

  private currentExplainContext(): ExplainContext | null {
    if (!this.provider || !this.activeDomain || !this.activeView || !this.selectedPosition) return null;
    return {
      provider: this.provider,
      fixtureId: this.fixtureId,
      domainId: this.activeDomain.id,
      position: this.selectedPosition,
      positionKey: positionKey(this.selectedPosition),
      viewId: this.activeView.id,
    };
  }

  private async loadExplain(context: ExplainContext): Promise<void> {
    if (this.destroyed) return;
    if (this.pendingExplain && sameExplainContext(context, this.pendingExplain)) return;
    if (this.snapshotExplain && sameExplainContext(context, this.snapshotExplain.context)) return;
    if (this.explainFailure && sameExplainContext(context, this.explainFailure.context)) return;
    const version = ++this.explainRequestVersion;
    this.pendingExplain = context;
    let chain: readonly ExplainStep[];
    try {
      chain = await context.provider.explain(context.position, context.viewId);
    } catch (error) {
      if (!this.isCurrentExplain(version, context)) return;
      this.pendingExplain = null;
      this.explainFailure = { context, failure: asProviderFailure(error) };
      this.showFailure(error, { kind: 'explanation', context });
      return;
    }
    if (!this.isCurrentExplain(version, context)) return;
    this.pendingExplain = null;
    this.explainFailure = null;
    this.snapshotExplain = { context, chain };
    const target = this.root.querySelector<HTMLElement>('.explain-tree');
    if (target) target.innerHTML = this.renderExplain(chain);
  }

  private isCurrentExplain(version: number, context: ExplainContext): boolean {
    const current = this.currentExplainContext();
    return !this.destroyed && version === this.explainRequestVersion && current !== null && sameExplainContext(context, current);
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
    this.syncPopoverTriggers();
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
    this.syncPopoverTriggers();
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
      this.syncPopoverTriggers();
      if (this.diagnosticsOpen) this.root.querySelector<HTMLInputElement>('#diagnostic-search')?.focus();
      return;
    }
    if (target.closest('#features-button')) {
      this.featuresOpen = !this.featuresOpen;
      this.debugOpen = false;
      this.diagnosticsOpen = false;
      this.renderDebugMenu(); this.renderDiagnostics(); this.renderFeatures();
      this.syncPopoverTriggers();
      return;
    }
    const viewButton = target.closest<HTMLElement>('[data-view-id]');
    if (viewButton?.dataset.viewId) { void this.selectView(viewButton.dataset.viewId); return; }
    const stageButton = target.closest<HTMLElement>('[data-stage-id]');
    if (stageButton?.dataset.stageId) { void this.selectStage(stageButton.dataset.stageId); return; }
    if (target.closest('.close-popover')) { this.closePopovers(true); return; }
    if (target.closest('.close-inspection')) { this.closeInspection(); return; }
    if (target.closest('#inspect-center')) { this.viewport.inspectCenter(); return; }
    if (target.closest('#retry-button')) { this.retryFailure(); return; }
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
      if (this.selectedPosition) { this.closeInspection(); return; }
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
  const plot = buildRadialProfilePlot(values);
  const sourceCount = formatCount(values.length);
  if (!plot) {
    const message = values.length < 2 ? 'No profile values supplied.' : 'No usable profile values supplied.';
    return `<div class="profile-chart"><p class="eyebrow">PROFILE · PROVIDER VALUES · ${sourceCount} samples</p><p class="muted">${message}</p></div>`;
  }
  return `<div class="profile-chart"><p class="eyebrow">PROFILE · PROVIDER VALUES · ${sourceCount} samples</p><svg class="profile-svg" viewBox="0 0 100 42" role="img" aria-label="Provider supplied radial profile with ${plot.sourceSampleCount} samples" data-source-sample-count="${plot.sourceSampleCount}" data-plotted-point-count="${plot.sampleIndices.length}" preserveAspectRatio="none"><polyline points="${plot.points}" fill="none" stroke="#c4c9cd" stroke-width="1.3" vector-effect="non-scaling-stroke"/><line x1="0" y1="39" x2="100" y2="39" stroke="#555d63" stroke-width="0.5" vector-effect="non-scaling-stroke"/></svg><div class="profile-axis"><span>Center</span><span>Outer sample</span></div></div>`;
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
function positionKey(position: PickPosition): string { return JSON.stringify(position); }
async function settle<T>(promise: Promise<T>): Promise<{ ok: true; value: T } | { ok: false; error: unknown }> {
  try { return { ok: true, value: await promise }; }
  catch (error) { return { ok: false, error }; }
}
