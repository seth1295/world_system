import '../src/styles.css';
import type { BodyCatalog, ExplainStep, FixtureOption, PickPosition, PointReport } from '../src/provider/contracts';
import { ProviderError } from '../src/provider/contracts';
import { MockBodyProvider } from '../src/provider/mock-provider';
import { InspectorApp } from '../src/ui/app';
import { buildFixture, SCENARIOS, type FixtureModel } from '../src/fixtures/scenarios';
import type { RegressionControlHandle } from './remediation-types';

const hostileColors = [
  '#fff" onmouseover="alert(1)',
  'red; background:url(javascript:alert(1))',
  '</div><script>alert(1)</script>',
  'rgb(1 2 / calc(',
  '',
  'var(--injected, url(javascript:alert(1)))',
  '#fff" onmouseover="window.hostileColorExecuted=true',
] as const;
const fixtureIds = ['fixture:category-heavy', 'fixture:multi-domain'] as const;
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

class ControlledProvider extends MockBodyProvider {
  constructor(fixture: FixtureModel, private readonly fixtureId: string, private readonly control: RegressionControl) {
    super(fixture);
  }

  override async inspect(position: PickPosition): Promise<PointReport> {
    const pointKey = JSON.stringify(position);
    return new Promise((resolve, reject) => {
      this.control.inspections.push({ fixtureId: this.fixtureId, positionKey: pointKey, resolve, reject, settled: false });
    });
  }

  override async explain(position: PickPosition, viewId: string): Promise<ExplainStep[]> {
    const pointKey = JSON.stringify(position);
    return new Promise((resolve, reject) => {
      this.control.explanations.push({ fixtureId: this.fixtureId, viewId, positionKey: pointKey, resolve, reject, settled: false });
    });
  }
}

class ControlledCatalog implements BodyCatalog {
  constructor(private readonly control: RegressionControl) {}

  fixtures(): readonly FixtureOption[] {
    return [...fixtureModels.values()].map(({ option }) => option);
  }

  async open(fixtureId: string): Promise<ControlledProvider> {
    const fixture = fixtureModels.get(fixtureId);
    if (!fixture) throw new Error(`Unknown regression fixture: ${fixtureId}`);
    return new ControlledProvider(fixture, fixtureId, this.control);
  }
}

interface RegressionControl extends RegressionControlHandle {
  explanations: DeferredExplain[];
  inspections: DeferredInspect[];
  rejectInspection(index: number, message: string): void;
}

const control: RegressionControl = {
  explanations: [],
  inspections: [],
  explanationCount(viewId, fixtureId) { return this.explanations.filter((request) => request.viewId === viewId && (!fixtureId || request.fixtureId === fixtureId)).length; },
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
const app = new InspectorApp(root, new ControlledCatalog(control));
control.pickPoint = (position) => {
  const appForTesting = app as unknown as { inspect(point: PickPosition): Promise<void> };
  void appForTesting.inspect(position);
};
window.addEventListener('pagehide', () => app.destroy(), { once: true });
void app.start();

function withHostileColors(fixture: FixtureModel): FixtureModel {
  const catalogs = new Map([...fixture.catalogs].map(([domainId, catalog]) => [domainId, {
    ...catalog,
    views: catalog.views.map((view) => {
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
      return {
        ...view,
        legend: {
          ...view.legend,
          stops: [...hostileColors.map((color, index) => ({ at: index / hostileColors.length, color })), ...view.legend.stops],
        },
      };
    }),
  }]));
  const tables = fixture.featureCatalog.tables.length
    ? fixture.featureCatalog.tables
    : [{ id: 'test-overlay', label: 'Provider overlay', count: 1, geometry: { color: hostileColors[0], paths: [[[-0.2, 0, 1], [0, 0.2, 1], [0.2, 0, 1]]] as const } }];
  return { ...fixture, catalogs, featureCatalog: { tables } };
}
