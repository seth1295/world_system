import '../src/styles.css';
import type { BodyCatalog, BodyProvider, DiagnosticSnapshot, DisplayTile, ExplainStep, FeatureCatalog, FixtureOption, PickPosition, PointReport, ViewCatalog, ViewStats } from '../src/provider/contracts';
import { ProviderError } from '../src/provider/contracts';
import { MockBodyProvider } from '../src/provider/mock-provider';
import { normalizeCssColor, normalizeCssColorWithAlpha, normalizePaletteStops, sampleNormalizedPalette } from '../src/render/css-color';
import * as THREE from 'three';
import type { Viewport } from '../src/render/viewport';
import { InspectorApp } from '../src/ui/app';
import { buildFixture, SCENARIOS, type FixtureModel } from '../src/fixtures/scenarios';
import type { OverlayMaterialState, RegressionControlHandle } from './remediation-types';

const hostileColors = [
  '#fff" onmouseover="alert(1)',
  'red; background:url(javascript:alert(1))',
  '</div><script>alert(1)</script>',
  'rgb(1 2 / calc(',
  '',
  'var(--injected, url(javascript:alert(1)))',
  '#fff" onmouseover="window.hostileColorExecuted=true',
] as const;
const fixtureIds = ['fixture:category-heavy', 'fixture:multi-domain', 'fixture:diagnostics', 'fixture:radial', 'fixture:normal-surface', 'fixture:void'] as const;
const fixtureModels = new Map<string, FixtureModel>(fixtureIds.map((id) => {
  const spec = SCENARIOS.find((candidate) => candidate.id === id);
  if (!spec) throw new Error(`Missing fixture ${id}`);
  const fixture = buildFixture(spec);
  return [id, withHostileColors(fixture)] as const;
}));

interface DeferredExplain {
  fixtureId: string;
  viewId: string;
  positionKey: string;
  resolve: (steps: ExplainStep[]) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

interface DeferredInspect {
  fixtureId: string;
  positionKey: string;
  resolve: (report: PointReport) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

interface DeferredStage {
  fixtureId: string;
  stageId: string;
  resolve: (snapshot: DiagnosticSnapshot) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

interface DeferredStats {
  fixtureId: string;
  viewId: string;
  resolve: (stats: ViewStats | undefined) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

interface DeferredTile {
  fixtureId: string;
  viewId: string;
  value: DisplayTile;
  resolve: (tile: DisplayTile) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

interface DeferredViews {
  fixtureId: string;
  domainId: string;
  value: ViewCatalog;
  resolve: (catalog: ViewCatalog) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

interface DeferredOpen {
  fixtureId: string;
  resolve: (provider: BodyProvider) => void;
  reject: (error: unknown) => void;
  settled: boolean;
}

class ControlledProvider extends MockBodyProvider {
  constructor(fixture: FixtureModel, private readonly fixtureId: string, private readonly control: RegressionControl) {
    super(fixture);
  }

  override async inspect(position: PickPosition): Promise<PointReport> {
    const pointKey = JSON.stringify(position);
    if (this.control.shouldFail('inspect', '*')) {
      this.control.inspections.push({ fixtureId: this.fixtureId, positionKey: pointKey, resolve: () => undefined, reject: () => undefined, settled: true });
      throw retryableFailure('inspection');
    }
    return new Promise((resolve, reject) => {
      this.control.inspections.push({ fixtureId: this.fixtureId, positionKey: pointKey, resolve, reject, settled: false });
    });
  }

  override async summary() {
    this.control.recordOperation('summary', this.fixtureId);
    if (this.control.shouldFail('summary', this.fixtureId) || this.control.shouldFail('summary', '*')) throw retryableFailure('summary');
    return super.summary();
  }

  override async stats(viewId: string): Promise<ViewStats | undefined> {
    this.control.recordOperation('stats', viewId);
    if (this.control.shouldFail('stats', viewId) || this.control.shouldFail('stats', '*')) throw retryableFailure('statistics');
    if (this.control.deferStats) {
      return new Promise((resolve, reject) => {
        this.control.statistics.push({ fixtureId: this.fixtureId, viewId, resolve, reject, settled: false });
      });
    }
    return super.stats(viewId);
  }

  override async explain(position: PickPosition, viewId: string): Promise<ExplainStep[]> {
    const pointKey = JSON.stringify(position);
    this.control.recordExplanation(this.fixtureId, viewId);
    if (this.control.shouldFail('explain', viewId) || this.control.shouldFail('explain', '*')) throw retryableFailure('explanation');
    return new Promise((resolve, reject) => {
      this.control.explanations.push({ fixtureId: this.fixtureId, viewId, positionKey: pointKey, resolve, reject, settled: false });
    });
  }

  override async tile(viewId: string, timeSelectionId?: string, diagnosticStageId?: string): Promise<DisplayTile> {
    this.control.recordOperation('tile', viewId);
    if (this.control.shouldFail('tile', viewId) || this.control.shouldFail('tile', '*')) throw retryableFailure('view data');
    const value = await super.tile(viewId, timeSelectionId, diagnosticStageId);
    if (!this.control.deferTiles) return value;
    return new Promise((resolve, reject) => {
      this.control.tiles.push({ fixtureId: this.fixtureId, viewId, value, resolve, reject, settled: false });
    });
  }

  override async features(): Promise<FeatureCatalog> {
    const catalog = await super.features();
    const color = this.control.overlayColor;
    if (color === null) return catalog;
    return {
      tables: catalog.tables.map((table) => table.geometry
        ? { ...table, geometry: { ...table.geometry, color } }
        : table),
    };
  }

  override async views(domainId: string): Promise<ViewCatalog> {
    this.control.recordOperation('views', domainId);
    if (this.control.shouldFail('views', domainId) || this.control.shouldFail('views', '*')) throw retryableFailure('domain catalogue');
    const value = await super.views(domainId);
    if (!this.control.deferViews) return value;
    return new Promise<ViewCatalog>((resolve, reject) => {
      this.control.viewCatalogs.push({ fixtureId: this.fixtureId, domainId, value, resolve, reject, settled: false });
    });
  }

  override async domainGeometry(domainId: string) {
    this.control.recordOperation('geometry', domainId);
    if (this.control.shouldFail('geometry', domainId) || this.control.shouldFail('geometry', '*')) throw retryableFailure('domain geometry');
    const geometry = await super.domainGeometry(domainId);
    const sampleCount = this.fixtureId === 'fixture:radial' ? this.control.largeProfileSamples : 0;
    if (!sampleCount || !geometry.profile) return geometry;
    const profile = Array.from({ length: sampleCount }, (_, index) => 0.5 + Math.sin(index * 0.0001) * 0.4);
    profile[0] = 0.1;
    profile[sampleCount - 1] = 0.9;
    profile[Math.floor(sampleCount * 0.2)] = -1;
    profile[Math.floor(sampleCount * 0.8)] = 1;
    return { ...geometry, profile };
  }

  override async diagnosticStage(stageId: string): Promise<DiagnosticSnapshot> {
    if (this.control.shouldFail('stage', stageId) || this.control.shouldFail('stage', '*')) throw retryableFailure('diagnostic stage');
    if (this.control.deferDiagnosticStages) {
      return new Promise((resolve, reject) => {
        this.control.stages.push({ fixtureId: this.fixtureId, stageId, resolve, reject, settled: false });
      });
    }
    return super.diagnosticStage(stageId);
  }
}

class ControlledCatalog implements BodyCatalog {
  constructor(private readonly control: RegressionControl) {}

  fixtures(): readonly FixtureOption[] {
    return [...fixtureModels.values()].map(({ option }) => option);
  }

  async open(fixtureId: string): Promise<ControlledProvider> {
    this.control.recordOperation('open', fixtureId);
    if (this.control.shouldFail('open', fixtureId)) throw retryableFailure('fixture load');
    const fixture = fixtureModels.get(fixtureId);
    if (!fixture) throw new Error(`Unknown regression fixture: ${fixtureId}`);
    if (this.control.deferOpen) {
      return new Promise((resolve, reject) => {
        this.control.opens.push({
          fixtureId,
          resolve: (provider) => resolve(provider as ControlledProvider),
          reject,
          settled: false,
        });
      });
    }
    return new ControlledProvider(fixture, fixtureId, this.control);
  }
}

interface ViewportProbe {
  root: THREE.Group;
  setData: Viewport['setData'];
  setOverlays: Viewport['setOverlays'];
  clearData: Viewport['clearData'];
}

interface AppRenderProbe {
  renderAll(missingResources?: readonly string[]): void;
  renderStatus(missingResources?: readonly string[]): void;
  renderError(): void;
  renderLegend(missingResources?: readonly string[]): void;
  renderInspection(): void;
}

interface RegressionControl extends RegressionControlHandle {
  explanations: DeferredExplain[];
  inspections: DeferredInspect[];
  stages: DeferredStage[];
  statistics: DeferredStats[];
  tiles: DeferredTile[];
  viewCatalogs: DeferredViews[];
  opens: DeferredOpen[];
  failures: Map<string, number>;
  explanationCounts: Map<string, number>;
  operationCounts: Map<string, number>;
  deferDiagnosticStages: boolean;
  deferStats: boolean;
  deferTiles: boolean;
  deferViews: boolean;
  deferOpen: boolean;
  overlayColor: string | null;
  destroyApp(): void;
  postDestroyViewportCalls(): number;
  unhandledRejectionCount(): number;
  overlayMaterialStates(): readonly OverlayMaterialState[];
  largeProfileSamples: number;
  failNext(operation: string, key: string): void;
  shouldFail(operation: string, key: string): boolean;
  recordExplanation(fixtureId: string, viewId: string): void;
  recordOperation(operation: string, key: string): void;
  setDiagnosticStageDeferred(value: boolean): void;
  diagnosticStageCount(stageId: string): number;
  resolveStage(stageId: string, label: string): void;
  rejectStage(stageId: string, message: string): void;
  normalizeColor(value: unknown): ReturnType<typeof normalizeCssColor>;
  samplePalette(stops: readonly { at: number; color: string }[], value: number): ReturnType<typeof sampleNormalizedPalette>;
  rejectInspection(index: number, message: string): void;
}

let appInstance: InspectorApp | null = null;
let viewportProbe: ViewportProbe | null = null;
let destroyedApp = false;
let postDestroyViewportCalls = 0;
let postDestroyRenderCalls = 0;
let unhandledRejections = 0;
window.addEventListener('unhandledrejection', () => { unhandledRejections += 1; });

const control: RegressionControl = {
  explanations: [],
  inspections: [],
  stages: [],
  statistics: [],
  tiles: [],
  viewCatalogs: [],
  opens: [],
  deferDiagnosticStages: false,
  deferStats: new URL(window.location.href).searchParams.get('deferStats') === '1',
  deferTiles: false,
  deferViews: false,
  deferOpen: new URL(window.location.href).searchParams.get('deferOpen') === '1',
  overlayColor: new URL(window.location.href).searchParams.has('overlayColor')
    ? new URL(window.location.href).searchParams.get('overlayColor') ?? ''
    : null,
  largeProfileSamples: Number(new URL(window.location.href).searchParams.get('profileSamples')) || 0,
  failures: new Map<string, number>(),
  explanationCounts: new Map<string, number>(),
  operationCounts: new Map<string, number>(),
  failNext(operation, key) { this.failures.set(`${operation}:${key}`, 1); },
  shouldFail(operation, key) {
    const failureKey = `${operation}:${key}`;
    const remaining = this.failures.get(failureKey) ?? 0;
    if (remaining <= 0) return false;
    this.failures.set(failureKey, remaining - 1);
    return true;
  },
  recordExplanation(fixtureId, viewId) {
    const key = `${fixtureId}:${viewId}`;
    this.explanationCounts.set(key, (this.explanationCounts.get(key) ?? 0) + 1);
  },
  recordOperation(operation, key) {
    const operationKey = `${operation}:${key}`;
    this.operationCounts.set(operationKey, (this.operationCounts.get(operationKey) ?? 0) + 1);
  },
  operationCount(operation, key) { return this.operationCounts.get(`${operation}:${key}`) ?? 0; },
  statsCount(viewId) { return this.operationCount('stats', viewId); },
  setDiagnosticStageDeferred(value) { this.deferDiagnosticStages = value; },
  setStatsDeferred(value) { this.deferStats = value; },
  resolveStats(viewId, min, max, mean) {
    const request = this.statistics.find((item) => !item.settled && item.viewId === viewId);
    if (!request) throw new Error(`No pending statistics request for ${viewId}`);
    request.settled = true;
    request.resolve({ min, max, mean, count: 7 });
  },
  rejectStats(viewId, message) {
    const request = this.statistics.find((item) => !item.settled && item.viewId === viewId);
    if (!request) throw new Error(`No pending statistics request for ${viewId}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_STATS_TEST', message, retryable: true, category: 'load' }));
  },
  setTilesDeferred(value) { this.deferTiles = value; },
  resolveTile(viewId) {
    const request = this.tiles.find((item) => !item.settled && item.viewId === viewId);
    if (!request) throw new Error(`No pending tile request for ${viewId}`);
    request.settled = true;
    request.resolve(request.value);
  },
  rejectTile(viewId, message) {
    const request = this.tiles.find((item) => !item.settled && item.viewId === viewId);
    if (!request) throw new Error(`No pending tile request for ${viewId}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_TILE_TEST', message, retryable: true, category: 'load' }));
  },
  setViewsDeferred(value) { this.deferViews = value; },
  resolveViews(domainId) {
    const request = this.viewCatalogs.find((item) => !item.settled && item.domainId === domainId);
    if (!request) throw new Error(`No pending view catalogue for ${domainId}`);
    request.settled = true;
    request.resolve(request.value);
  },
  rejectViews(domainId, message) {
    const request = this.viewCatalogs.find((item) => !item.settled && item.domainId === domainId);
    if (!request) throw new Error(`No pending view catalogue for ${domainId}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_VIEWS_TEST', message, retryable: true, category: 'load' }));
  },
  resolveOpen(fixtureId) {
    const request = this.opens.find((item) => !item.settled && item.fixtureId === fixtureId);
    if (!request) throw new Error(`No pending open request for ${fixtureId}`);
    request.settled = true;
    const fixture = fixtureModels.get(fixtureId);
    if (!fixture) throw new Error(`Missing fixture for open request ${fixtureId}`);
    request.resolve(new ControlledProvider(fixture, fixtureId, this));
  },
  rejectOpen(fixtureId, message) {
    const request = this.opens.find((item) => !item.settled && item.fixtureId === fixtureId);
    if (!request) throw new Error(`No pending open request for ${fixtureId}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_OPEN_TEST', message, retryable: true, category: 'load' }));
  },
  destroyApp() {
    if (destroyedApp) return;
    destroyedApp = true;
    appInstance?.destroy();
  },
  postDestroyViewportCalls() { return postDestroyViewportCalls; },
  postDestroyRenderCalls() { return postDestroyRenderCalls; },
  unhandledRejectionCount() { return unhandledRejections; },
  setLargeProfileSamples(count) { this.largeProfileSamples = count; },
  diagnosticStageCount(stageId) { return this.stages.filter((request) => request.stageId === stageId).length; },
  normalizeColor(value) { return normalizeCssColor(value); },
  normalizeOverlayColor(value) { return normalizeCssColorWithAlpha(value); },
  overlayMaterialStates() {
    return (viewportProbe?.root.children ?? []).flatMap((object): OverlayMaterialState[] => {
      if (!(object instanceof THREE.Line)) return [];
      const material = object.material;
      if (!(material instanceof THREE.LineBasicMaterial)) return [];
      return [{ color: `#${material.color.getHexString()}`, opacity: material.opacity, transparent: material.transparent }];
    });
  },
  samplePalette(stops, value) { return sampleNormalizedPalette(normalizePaletteStops(stops), value); },
  resolveStage(stageId, label) {
    const request = this.stages.find((item) => !item.settled && item.stageId === stageId);
    if (!request) throw new Error(`No pending diagnostic stage ${stageId}`);
    request.settled = true;
    request.resolve({ stageId, message: `Harness snapshot ${label}.`, values: [{ label: 'Stage', value: label }] });
  },
  rejectStage(stageId, message) {
    const request = this.stages.find((item) => !item.settled && item.stageId === stageId);
    if (!request) throw new Error(`No pending diagnostic stage ${stageId}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_STAGE_TEST', message, retryable: false, category: 'load' }));
  },
  explanationCount(viewId, fixtureId) {
    if (fixtureId) return this.explanationCounts.get(`${fixtureId}:${viewId}`) ?? 0;
    return [...this.explanationCounts].filter(([key]) => key.endsWith(`:${viewId}`)).reduce((sum, [, count]) => sum + count, 0);
  },
  inspectionCount() { return this.inspections.length; },
  inspectionPositionKey(index) { return this.inspections[index]?.positionKey; },
  pickPoint() { throw new Error('Inspector is not ready for point selection'); },
  resolveExplain(viewId, label, fixtureId) {
    const request = this.explanations.find((item) => !item.settled && item.viewId === viewId && (!fixtureId || item.fixtureId === fixtureId));
    if (!request) throw new Error(`No pending explanation for ${viewId}`);
    request.settled = true;
    request.resolve([{ id: `result-${label}`, label, description: `Resolved explanation ${label}.` }]);
  },
  rejectExplain(viewId, message, fixtureId) {
    const request = this.explanations.find((item) => !item.settled && item.viewId === viewId && (!fixtureId || item.fixtureId === fixtureId));
    if (!request) throw new Error(`No pending explanation for ${viewId}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_EXPLAIN_TEST', message, retryable: false, category: 'load' }));
  },
  resolveInspection(index, label) {
    const request = this.inspections.filter((item) => !item.settled)[index];
    if (!request) throw new Error(`No pending inspection at index ${index}`);
    request.settled = true;
    const fixture = fixtureModels.get(request.fixtureId);
    if (!fixture) throw new Error(`Missing fixture for inspection ${request.fixtureId}`);
    request.resolve({ ...fixture.pointReport, positionLabel: label, positionValue: request.positionKey });
  },
  rejectInspection(index, message) {
    const request = this.inspections.filter((item) => !item.settled)[index];
    if (!request) throw new Error(`No pending inspection at index ${index}`);
    request.settled = true;
    request.reject(new ProviderError({ code: 'E_INSPECT_TEST', message, retryable: false, category: 'load' }));
  },
};

const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Regression harness root is missing');
window.remediationControl = control;
appInstance = new InspectorApp(root, new ControlledCatalog(control));
viewportProbe = (appInstance as unknown as { viewport: ViewportProbe }).viewport;
const appRenderProbe = appInstance as unknown as AppRenderProbe;
const originalRenderAll = appRenderProbe.renderAll.bind(appRenderProbe);
appRenderProbe.renderAll = (missingResources) => {
  if (destroyedApp) postDestroyRenderCalls += 1;
  originalRenderAll(missingResources);
};
const originalRenderStatus = appRenderProbe.renderStatus.bind(appRenderProbe);
appRenderProbe.renderStatus = (missingResources) => {
  if (destroyedApp) postDestroyRenderCalls += 1;
  originalRenderStatus(missingResources);
};
const originalRenderError = appRenderProbe.renderError.bind(appRenderProbe);
appRenderProbe.renderError = () => {
  if (destroyedApp) postDestroyRenderCalls += 1;
  originalRenderError();
};
const originalRenderLegend = appRenderProbe.renderLegend.bind(appRenderProbe);
appRenderProbe.renderLegend = (missingResources) => {
  if (destroyedApp) postDestroyRenderCalls += 1;
  originalRenderLegend(missingResources);
};
const originalRenderInspection = appRenderProbe.renderInspection.bind(appRenderProbe);
appRenderProbe.renderInspection = () => {
  if (destroyedApp) postDestroyRenderCalls += 1;
  originalRenderInspection();
};
const originalSetData = viewportProbe.setData.bind(viewportProbe);
viewportProbe.setData = (geometry, descriptor, tile) => {
  if (destroyedApp) postDestroyViewportCalls += 1;
  originalSetData(geometry, descriptor, tile);
};
const originalSetOverlays = viewportProbe.setOverlays.bind(viewportProbe);
viewportProbe.setOverlays = (tables) => {
  if (destroyedApp) postDestroyViewportCalls += 1;
  originalSetOverlays(tables);
};
const originalClearData = viewportProbe.clearData.bind(viewportProbe);
viewportProbe.clearData = () => {
  if (destroyedApp) postDestroyViewportCalls += 1;
  originalClearData();
};
control.pickPoint = (position) => {
  const appForTesting = appInstance as unknown as { inspect(point: PickPosition): Promise<void> };
  void appForTesting.inspect(position);
};
window.addEventListener('pagehide', () => control.destroyApp(), { once: true });
void appInstance.start();

function withHostileColors(fixture: FixtureModel): FixtureModel {
  const catalogs = new Map([...fixture.catalogs].map(([domainId, catalog]) => [domainId, {
    ...catalog,
    views: catalog.views.map((view, viewIndex) => {
      if (view.legend.kind === 'categorical') {
        return {
          ...view,
          legend: {
            ...view.legend,
            categories: view.legend.categories.map((category, index) => ({
              ...category,
              color: hostileColors[index] ?? category.color,
            })),
          },
        };
      }
      const stops = fixture.option.id === 'fixture:category-heavy' && viewIndex === 2
        ? [{ at: 0, color: 'rgb(255 0 0)' }, { at: 0.5, color: 'hsl(240 100% 50%)' }, { at: 1, color: 'blue' }]
        : fixture.option.id === 'fixture:category-heavy' && viewIndex === 3
          ? [{ at: 0, color: '#ff0000' }, { at: 0.5, color: '#f00' }, { at: 1, color: 'red' }]
          : [...hostileColors.map((color, index) => ({ at: index / hostileColors.length, color })), ...view.legend.stops];
      return {
        ...view,
        legend: {
          ...view.legend,
          stops,
        },
        ...(fixture.option.id === 'fixture:multi-domain'
          ? { timeSelections: [{ id: 'mean', label: 'Mean' }, { id: 'slice-2', label: 'Slice 02' }], defaultTimeSelectionId: 'mean' }
          : {}),
      };
    }),
  }]));
  const tables = fixture.featureCatalog.tables.length
    ? fixture.featureCatalog.tables
    : [{ id: 'test-overlay', label: 'Provider overlay', count: 1, geometry: { color: hostileColors[0], paths: [[[-0.2, 0, 1], [0, 0.2, 1], [0.2, 0, 1]]] as const } }];
  const overlayColor = new URL(window.location.href).searchParams.get('overlayColor');
  const overlayTables = overlayColor === null
    ? tables
    : tables.map((table) => table.geometry ? { ...table, geometry: { ...table.geometry, color: overlayColor } } : table);
  return { ...fixture, catalogs, featureCatalog: { tables: overlayTables } };
}

function retryableFailure(operation: string): ProviderError {
  return new ProviderError({
    code: 'E_RETRYABLE_TEST',
    message: `Temporary ${operation} failure.`,
    retryable: true,
    category: 'load',
  });
}
