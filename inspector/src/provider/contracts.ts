export type Vec3 = readonly [number, number, number];

export interface FixtureOption {
  id: string;
  label: string;
  description: string;
}

export interface BodySummary {
  objectId: string;
  name: string;
}

export type RenderKind = 'surface-mesh' | 'radial-profile';

export interface DomainDescriptor {
  id: string;
  label: string;
  topology: string;
  renderKind: RenderKind;
}

export interface ViewGroupDescriptor {
  id: string;
  label: string;
  order: number;
}

export interface PaletteStop {
  at: number;
  color: string;
}

export interface ContinuousLegend {
  kind: 'continuous';
  unit?: string;
  stops: readonly PaletteStop[];
  statsAvailable: boolean;
  range?: { min: number; max: number };
}

export interface CategoryLegendEntry {
  label: string;
  color: string;
  count?: number;
  weightedPercent?: number;
}

export interface CategoricalLegend {
  kind: 'categorical';
  unit?: string;
  categories: readonly CategoryLegendEntry[];
}

export type LegendDescriptor = ContinuousLegend | CategoricalLegend;

export interface TimeSelection {
  id: string;
  label: string;
}

export interface ViewDescriptor {
  id: string;
  fieldOrDerivedId: string;
  operator: string;
  domainId: string;
  groupId: string;
  label: string;
  description: string;
  legend: LegendDescriptor;
  timeSelections?: readonly TimeSelection[];
  defaultTimeSelectionId?: string;
}

export interface ViewCatalog {
  groups: readonly ViewGroupDescriptor[];
  views: readonly ViewDescriptor[];
}

export interface ViewStats {
  min: number;
  max: number;
  mean: number;
  count?: number;
}

export interface RenderGeometry {
  kind: RenderKind;
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  indices: Uint32Array;
  profile?: readonly number[];
}

export interface DisplayTile {
  width: number;
  height: number;
  values: Float32Array;
  /** Fixture completeness is supplied by the provider; it is not inferred from tile values. */
  missingResources?: readonly string[];
}

export type PickPosition =
  | { kind: 'surface-direction'; direction: Vec3 }
  | { kind: 'radial-distance'; normalizedRadius: number };

export interface PointField {
  id: string;
  label: string;
  value?: string | null;
  unit?: string | null;
  nodata?: boolean;
  sourceKind: string;
  levelUsed?: string | number | null;
}

export interface PointFieldGroup {
  id: string;
  label: string;
  fields: readonly PointField[];
}

export interface PointReport {
  positionLabel: string;
  positionValue: string;
  groups: readonly PointFieldGroup[];
}

export interface ExplainReference {
  label: string;
  value: string;
}

export interface ExplainStep {
  id: string;
  label: string;
  description: string;
  references?: readonly ExplainReference[];
  children?: readonly ExplainStep[];
}

export interface FeatureGeometry {
  paths: readonly (readonly Vec3[])[];
  color: string;
}

export interface FeatureTable {
  id: string;
  label: string;
  count: number;
  geometry?: FeatureGeometry;
}

export interface FeatureCatalog {
  tables: readonly FeatureTable[];
}

export interface DiagnosticStage {
  id: string;
  label: string;
}

export type DiagnosticsDescriptor =
  | { available: false }
  | { available: true; stages: readonly DiagnosticStage[] };

export interface DiagnosticSnapshot {
  stageId: string;
  message: string;
  values: readonly { label: string; value: string }[];
}

export interface ProviderFailure {
  code: string;
  message: string;
  offendingItem?: string;
  retryable: boolean;
  category: 'missing-content' | 'validation' | 'unsupported' | 'load' | 'unknown';
}

export class ProviderError extends Error {
  constructor(readonly failure: ProviderFailure) {
    super(failure.message);
    this.name = 'ProviderError';
  }
}

export function asProviderFailure(error: unknown): ProviderFailure {
  if (error instanceof ProviderError) return error.failure;
  return { code: 'E_PROVIDER_UNKNOWN', message: error instanceof Error ? error.message : 'The provider returned an unknown error.', retryable: false, category: 'unknown' };
}

export interface BodyProvider {
  summary(): Promise<BodySummary>;
  domains(): Promise<readonly DomainDescriptor[]>;
  views(domainId: string): Promise<ViewCatalog>;
  stats(viewId: string): Promise<ViewStats | undefined>;
  domainGeometry(domainId: string): Promise<RenderGeometry>;
  tile(viewId: string, timeSelectionId?: string, diagnosticStageId?: string): Promise<DisplayTile>;
  inspect(position: PickPosition): Promise<PointReport>;
  explain(position: PickPosition, viewId: string): Promise<readonly ExplainStep[]>;
  features(): Promise<FeatureCatalog>;
  diagnostics(): Promise<DiagnosticsDescriptor>;
  diagnosticStage(stageId: string): Promise<DiagnosticSnapshot>;
}

/** The UI depends on this seam. Fixture construction stays behind MockBodyCatalog. */
export interface BodyCatalog {
  fixtures(): readonly FixtureOption[];
  open(fixtureId: string): Promise<BodyProvider>;
}
