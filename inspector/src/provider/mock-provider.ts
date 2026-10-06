import type {
  BodyCatalog,
  BodyProvider,
  DiagnosticSnapshot,
  DiagnosticsDescriptor,
  DisplayTile,
  FeatureCatalog,
  ExplainStep,
  FixtureOption,
  PickPosition,
  RenderGeometry,
  ViewCatalog,
  ViewStats,
} from './contracts';
import { ProviderError } from './contracts';
import { FIXTURE_OPTIONS, SCENARIOS, buildFixture, buildGeometry, type FixtureModel } from '../fixtures/scenarios';

const frame = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** Synthetic provider used to stress UI assumptions; every response is presentation test data. */
export class MockBodyCatalog implements BodyCatalog {
  private readonly fixtureModels = new Map(SCENARIOS.map((spec) => [spec.id, buildFixture(spec)]));
  private readonly attempts = new Map<string, number>();

  fixtures(): readonly FixtureOption[] {
    return FIXTURE_OPTIONS;
  }

  async open(fixtureId: string): Promise<BodyProvider> {
    const fixture = this.fixtureModels.get(fixtureId);
    if (!fixture) throw new ProviderError({ code: 'E_FIXTURE_UNKNOWN', message: `Fixture “${fixtureId}” is not in the catalogue.`, retryable: false, category: 'unknown' });
    const attempt = (this.attempts.get(fixtureId) ?? 0) + 1;
    this.attempts.set(fixtureId, attempt);
    if (fixture.openingDelayMs > 0) await frame(fixture.openingDelayMs);
    if (fixture.failFirstOpen && attempt === 1) throw new ProviderError(fixture.failFirstOpen);
    if (fixture.openFailure) throw new ProviderError(fixture.openFailure);
    return new MockBodyProvider(fixture);
  }
}

export class MockBodyProvider implements BodyProvider {
  constructor(private readonly fixture: FixtureModel) {}

  async summary() {
    if (this.fixture.metadataDelayMs > 0) await frame(this.fixture.metadataDelayMs);
    if (this.fixture.metadataFailure) throw new ProviderError(this.fixture.metadataFailure);
    return this.fixture.summary;
  }

  async domains() {
    if (this.fixture.metadataDelayMs > 0) await frame(this.fixture.metadataDelayMs);
    return this.fixture.domains;
  }

  async views(domainId: string): Promise<ViewCatalog> {
    const catalog = this.fixture.catalogs.get(domainId);
    if (!catalog) throw new ProviderError({ code: 'E_DOMAIN_UNKNOWN', message: 'The selected domain is not available in this fixture.', offendingItem: domainId, retryable: false, category: 'unknown' });
    return catalog;
  }

  async stats(viewId: string): Promise<ViewStats | undefined> {
    const view = this.findView(viewId);
    if (!view || view.legend.kind !== 'continuous' || !view.legend.statsAvailable) return undefined;
    if (view.legend.range) return { min: view.legend.range.min, max: view.legend.range.max, mean: 4.21e150, count: 12_345_678 };
    const salt = hash(viewId) + this.fixture.seed;
    return { min: -salt / 17, max: salt * 2.37, mean: salt / 3.11, count: Math.floor(salt * 137) };
  }

  async domainGeometry(domainId: string): Promise<RenderGeometry> {
    const domainIndex = this.fixture.domains.findIndex(({ id }) => id === domainId);
    const domain = this.fixture.domains[domainIndex];
    if (!domain) throw new ProviderError({ code: 'E_DOMAIN_UNKNOWN', message: 'The selected domain is not available in this fixture.', offendingItem: domainId, retryable: false, category: 'unknown' });
    const sourceSeed = SCENARIOS.find(({ id }) => id === this.fixture.option.id);
    const sourceDomain = sourceSeed?.domains?.[domainIndex];
    return buildGeometry(domain.renderKind, this.fixture.seed + domainIndex * 29, sourceDomain?.deformed ?? false);
  }

  async tile(viewId: string, timeSelectionId?: string, diagnosticStageId?: string): Promise<DisplayTile> {
    if (this.fixture.viewDelayMs > 0) await frame(this.fixture.viewDelayMs);
    if (this.fixture.viewFailure) throw new ProviderError(this.fixture.viewFailure);
    const descriptor = this.findView(viewId);
    if (!descriptor) throw new ProviderError({ code: 'E_VIEW_UNKNOWN', message: 'The selected view is not available in this domain.', offendingItem: viewId, retryable: false, category: 'unknown' });
    const width = 192;
    const height = 96;
    const values = new Float32Array(width * height);
    const channel = hash(`${viewId}:${timeSelectionId ?? 'default'}:${diagnosticStageId ?? 'default'}`);
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        // Deliberately plain presentation data: a repeatable ramp makes view and
        // legend changes visible without drawing realistic looking surface maps.
        const horizontal = x / (width - 1);
        const vertical = y / (height - 1);
        values[y * width + x] = (horizontal * 0.68 + vertical * 0.32 + channel * 0.173) % 1;
      }
    }
    const missingResources = this.fixture.missingResources;
    return { width, height, values, ...(missingResources.length ? { missingResources } : {}) };
  }

  async inspect(position: PickPosition) {
    void position;
    return this.fixture.pointReport;
  }

  async explain(_position: PickPosition, viewId: string) {
    const view = this.findView(viewId);
    if (!view) return [];
    let node: ExplainStep = {
      id: `explain-${this.fixture.explainDepth}`,
      label: this.fixture.longExplain ? `Provider response ${'with extended explanation text '.repeat(3)}${this.fixture.explainDepth}` : `Provider response ${this.fixture.explainDepth}`,
      description: this.fixture.longExplain ? `Synthetic explanation detail ${'describing fixture provenance only '.repeat(12)}` : 'Synthetic fixture response text for interface testing.',
      references: this.fixture.longExplain ? [{ label: 'Reference', value: `fixture:reference/${'x'.repeat(80)}` }] : [{ label: 'Reference', value: `fixture:${view.id}` }],
    };
    for (let level = this.fixture.explainDepth - 1; level >= 1; level -= 1) {
      node = {
        id: `explain-${level}`,
        label: this.fixture.longExplain ? `Explanation stage ${level} ${'provider supplied long label '.repeat(3)}` : `Explanation stage ${level}`,
        description: this.fixture.longExplain ? `${'Synthetic nested explanation text. '.repeat(10)}Depth ${level}.` : `Synthetic fixture explanation at depth ${level}.`,
        references: [{ label: 'Input', value: `${view.id} · ${level}` }],
        children: [node],
      };
    }
    return [node];
  }

  async features(): Promise<FeatureCatalog> {
    return this.fixture.featureCatalog;
  }

  async diagnostics(): Promise<DiagnosticsDescriptor> {
    if (this.fixture.diagnosticStages === null || this.fixture.diagnosticStages.length === 0) return { available: false };
    return { available: true, stages: this.fixture.diagnosticStages };
  }

  async diagnosticStage(stageId: string): Promise<DiagnosticSnapshot> {
    const stage = this.fixture.diagnosticStages?.find(({ id }) => id === stageId);
    if (!stage) throw new ProviderError({ code: 'E_STAGE_UNKNOWN', message: 'The selected diagnostic stage is not available.', offendingItem: stageId, retryable: false, category: 'unknown' });
    const index = Number(stageId.split('-').at(-1) ?? '0');
    return {
      stageId,
      message: `Provider snapshot for ${stage.label}.`,
      values: [
        { label: 'State', value: `Synthetic state ${index}` },
        { label: 'Available resources', value: String(Math.max(0, this.fixture.domains.length * 4 - index)) },
      ],
    };
  }

  private findView(viewId: string) {
    for (const catalog of this.fixture.catalogs.values()) {
      const view = catalog.views.find(({ id }) => id === viewId);
      if (view) return view;
    }
    return undefined;
  }
}

function hash(value: string): number {
  let result = 2166136261;
  for (let index = 0; index < value.length; index += 1) result = Math.imul(result ^ value.charCodeAt(index), 16777619);
  return result >>> 0;
}
