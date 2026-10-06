import type {
  BodySummary,
  DiagnosticStage,
  DomainDescriptor,
  FeatureCatalog,
  FixtureOption,
  PointReport,
  RenderGeometry,
  ViewCatalog,
  ViewDescriptor,
  ViewGroupDescriptor,
} from '../provider/contracts';
import type { ProviderFailure } from '../provider/contracts';

export interface FixtureDomainSeed {
  id: string;
  label: string;
  topology: string;
  renderKind: DomainDescriptor['renderKind'];
  groups: number;
  viewCounts?: readonly number[];
  groupLabels?: readonly string[];
  longNames?: boolean;
  deformed?: boolean;
}

export interface FixtureSpec {
  id: string;
  label: string;
  name: string;
  description: string;
  seed: number;
  domains?: readonly FixtureDomainSeed[];
  groups?: number;
  viewsPerGroup?: number;
  viewCounts?: readonly number[];
  groupLabels?: readonly string[];
  longNames?: boolean;
  temporalSlices?: readonly number[];
  longTemporalLabels?: boolean;
  categoryCount?: number;
  longCategoryLabels?: boolean;
  unevenCounts?: boolean;
  includeUnit?: boolean;
  includeStats?: boolean;
  hugeRange?: boolean;
  pointFields?: number;
  longPointLabels?: boolean;
  explainDepth?: number;
  longExplain?: boolean;
  diagnosticStages?: number | null;
  longDiagnosticNames?: boolean;
  featureTables?: number;
  geometryEvery?: number;
  missingResources?: readonly string[];
  openingDelayMs?: number;
  metadataDelayMs?: number;
  viewDelayMs?: number;
  openFailure?: ProviderFailure;
  failFirstOpen?: ProviderFailure;
  metadataFailure?: ProviderFailure;
  viewFailure?: ProviderFailure;
}

export interface FixtureModel {
  option: FixtureOption;
  summary: BodySummary;
  domains: readonly DomainDescriptor[];
  catalogs: ReadonlyMap<string, ViewCatalog>;
  pointReport: PointReport;
  explainDepth: number;
  longExplain: boolean;
  diagnosticStages: readonly DiagnosticStage[] | null;
  featureCatalog: FeatureCatalog;
  seed: number;
  missingResources: readonly string[];
  openingDelayMs: number;
  metadataDelayMs: number;
  viewDelayMs: number;
  openFailure?: ProviderFailure;
  failFirstOpen?: ProviderFailure;
  metadataFailure?: ProviderFailure;
  viewFailure?: ProviderFailure;
}

const noContent: ProviderFailure = {
  code: 'E_CONTENT_MISSING', message: 'A required fixture resource is not available.', offendingItem: 'fixture/body-metadata', retryable: false, category: 'missing-content',
};
const invalid: ProviderFailure = {
  code: 'E_VALIDATION_DESCRIPTOR', message: 'The fixture provider reported a descriptor validation failure.', offendingItem: 'view:fixture-invalid', retryable: false, category: 'validation',
};
const unsupported: ProviderFailure = {
  code: 'E_CRITICAL_FEATURE_UNSUPPORTED', message: 'This fixture requires a critical feature that this Inspector cannot display.', offendingItem: 'feature:critical-v2', retryable: false, category: 'unsupported',
};
const retryable: ProviderFailure = {
  code: 'E_RESOURCE_TEMPORARY', message: 'The fixture resource could not be loaded. Retry to request it again.', offendingItem: 'fixture/resource-01', retryable: true, category: 'load',
};
const permanent: ProviderFailure = {
  code: 'E_RESOURCE_UNAVAILABLE', message: 'The fixture resource is unavailable and cannot be retried.', offendingItem: 'fixture/resource-02', retryable: false, category: 'load',
};

const surfaceDomain = (groups: number, groupLabels?: readonly string[], viewCounts?: readonly number[], longNames = false): FixtureDomainSeed => ({
  id: 'domain-surface', label: 'Surface domain', topology: 'dir_cube/1', renderKind: 'surface-mesh', groups,
  ...(groupLabels ? { groupLabels } : {}), ...(viewCounts ? { viewCounts } : {}), longNames,
});

/** Scenario recipes are deliberately compact; composable builders below expand them into test data. */
export const SCENARIOS: readonly FixtureSpec[] = [
  { id: 'fixture:void', label: 'void', name: 'Empty fixture', description: 'No domain or view descriptors.', seed: 1, domains: [], groups: 0, pointFields: 0 },
  { id: 'fixture:minimal', label: 'minimal', name: 'Minimal fixture', description: 'One domain and one declared view.', seed: 2, domains: [surfaceDomain(1, ['Minimal group'], [1])], pointFields: 1, explainDepth: 1 },
  { id: 'fixture:normal-surface', label: 'normal-surface', name: 'Veyra', description: 'Synthetic descriptor set with five declared groups.', seed: 3, domains: [surfaceDomain(5, ['Spatial', 'Topography', 'Tectonics', 'Ocean', 'Climate'], [4, 4, 4, 4, 4])], pointFields: 6, featureTables: 2, geometryEvery: 1 },
  { id: 'fixture:radial', label: 'radial', name: 'Auren', description: 'Radial-profile descriptor set.', seed: 4, domains: [{ id: 'domain-interior', label: 'Interior profile', topology: 'radial_1d/1', renderKind: 'radial-profile', groups: 1, groupLabels: ['Stellar structure'], viewCounts: [4] }], pointFields: 5 },
  { id: 'fixture:irregular', label: 'irregular', name: 'Irregular Rock', description: 'A plain irregular mesh with declared surface views.', seed: 5, domains: [{ id: 'domain-surface', label: 'Surface domain', topology: 'mesh/1', renderKind: 'surface-mesh', groups: 4, groupLabels: ['Spatial', 'Solid surface', 'Surface material', 'Thermal state'], viewCounts: [1, 1, 1, 1], deformed: true }], pointFields: 4 },
  { id: 'fixture:view-catalog-extreme', label: 'view-catalog-extreme', name: 'Large catalogue fixture', description: '72 views in 12 groups with long labels.', seed: 6, domains: [surfaceDomain(12, undefined, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 1, 16], true)], longNames: true, pointFields: 4 },
  { id: 'fixture:temporal-heavy', label: 'temporal-heavy', name: 'Time selection fixture', description: 'Several descriptors with many time choices.', seed: 7, domains: [surfaceDomain(3, ['Periodic set A', 'Periodic set B', 'Periodic set C'], [1, 1, 1])], temporalSlices: [12, 24, 52], longTemporalLabels: true, pointFields: 3 },
  { id: 'fixture:category-heavy', label: 'category-heavy', name: 'Legend fixture', description: 'Dense categorical and continuous legend descriptors.', seed: 8, domains: [surfaceDomain(1, ['Legend samples'], [4])], categoryCount: 36, longCategoryLabels: true, unevenCounts: true, includeUnit: false, includeStats: false, hugeRange: true, pointFields: 2 },
  { id: 'fixture:point-heavy', label: 'point-heavy', name: 'Point report fixture', description: 'A long mixed-source report with 46 fields.', seed: 9, domains: [surfaceDomain(3)], pointFields: 46, longPointLabels: true, explainDepth: 4 },
  { id: 'fixture:provenance-heavy', label: 'provenance-heavy', name: 'Explanation fixture', description: 'A deeply nested explanation with long references.', seed: 10, domains: [surfaceDomain(2)], pointFields: 8, explainDepth: 12, longExplain: true },
  { id: 'fixture:diagnostics', label: 'diagnostics', name: 'Diagnostic fixture', description: 'Sixteen provider-supplied diagnostic stages.', seed: 11, domains: [surfaceDomain(2)], diagnosticStages: 16, longDiagnosticNames: true, pointFields: 3 },
  { id: 'fixture:diagnostics-unavailable', label: 'diagnostics-unavailable', name: 'No diagnostic fixture', description: 'Diagnostics are not available.', seed: 12, domains: [surfaceDomain(1)], diagnosticStages: null },
  { id: 'fixture:diagnostics-single', label: 'diagnostics-single', name: 'Single-stage fixture', description: 'One diagnostic stage.', seed: 13, domains: [surfaceDomain(1)], diagnosticStages: 1 },
  { id: 'fixture:multi-domain', label: 'multi-domain', name: 'Multiple domain fixture', description: 'Three domains with independent view catalogues.', seed: 14, domains: [surfaceDomain(2, ['First capability', 'Second capability'], [2, 3]), { id: 'domain-profile', label: 'Profile domain', topology: 'radial_1d/1', renderKind: 'radial-profile', groups: 2, groupLabels: ['Profile group A', 'Profile group B'], viewCounts: [2, 1] }, { id: 'domain-mesh', label: 'Mesh domain', topology: 'mesh/1', renderKind: 'surface-mesh', groups: 1, groupLabels: ['Mesh capability'], viewCounts: [3], longNames: true }], pointFields: 5 },
  { id: 'fixture:features', label: 'features', name: 'Overlay catalogue fixture', description: 'Forty provider-supplied overlay tables, some with geometry.', seed: 15, domains: [surfaceDomain(2)], featureTables: 40, geometryEvery: 3 },
  { id: 'fixture:features-no-geometry', label: 'features-no-geometry', name: 'Non-geometric overlay fixture', description: 'Overlay tables are present without display geometry.', seed: 16, domains: [surfaceDomain(1)], featureTables: 18, geometryEvery: 0 },
  { id: 'fixture:features-none', label: 'features-none', name: 'No overlay fixture', description: 'No overlay tables are declared.', seed: 17, domains: [surfaceDomain(1)], featureTables: 0 },
  { id: 'fixture:partial-data', label: 'partial-data', name: 'Partial resource fixture', description: 'Some view resources are missing; available data remains visible.', seed: 18, domains: [surfaceDomain(2)], missingResources: ['tile/03', 'metadata/index-02'] },
  { id: 'fixture:loading', label: 'loading', name: 'Loading fixture', description: 'Opening and metadata requests are intentionally delayed.', seed: 19, domains: [surfaceDomain(2)], openingDelayMs: 1200, metadataDelayMs: 900, viewDelayMs: 1200 },
  { id: 'fixture:view-loading', label: 'view-loading', name: 'View loading fixture', description: 'View requests are intentionally delayed.', seed: 20, domains: [surfaceDomain(2)], viewDelayMs: 1600 },
  { id: 'fixture:missing-content', label: 'missing-content', name: 'Missing content fixture', description: 'Opening reports an unavailable required resource.', seed: 21, openFailure: noContent },
  { id: 'fixture:validation-failure', label: 'validation-failure', name: 'Validation failure fixture', description: 'Opening reports a provider validation result.', seed: 22, metadataFailure: invalid },
  { id: 'fixture:unsupported-critical', label: 'unsupported-critical', name: 'Unsupported feature fixture', description: 'Opening refuses a critical unsupported requirement.', seed: 23, openFailure: unsupported },
  { id: 'fixture:retryable-error', label: 'retryable-error', name: 'Retry fixture', description: 'The first request fails and a retry succeeds.', seed: 24, failFirstOpen: retryable },
  { id: 'fixture:non-retryable-error', label: 'non-retryable-error', name: 'Permanent error fixture', description: 'A non-retryable provider failure.', seed: 25, openFailure: permanent },
];

export const FIXTURE_OPTIONS: readonly FixtureOption[] = SCENARIOS.map(({ id, label, description }) => ({ id, label, description }));

export function buildFixture(spec: FixtureSpec): FixtureModel {
  const seeds = spec.domains ?? [surfaceDomain(spec.groups ?? 2, spec.groupLabels, spec.viewCounts, spec.longNames)];
  const domains: DomainDescriptor[] = seeds.map(({ id, label, topology, renderKind }) => ({ id, label, topology, renderKind }));
  const catalogs = new Map<string, ViewCatalog>();
  for (const [domainIndex, domain] of seeds.entries()) {
    catalogs.set(domain.id, buildViewCatalog(domain, spec, domainIndex));
  }
  const summary: BodySummary = {
    objectId: spec.id.replace('fixture:', 'fixture:object/'),
    name: spec.name,
  };
  return {
    option: { id: spec.id, label: spec.label, description: spec.description },
    summary,
    domains,
    catalogs,
    pointReport: buildPointReport(spec),
    explainDepth: spec.explainDepth ?? 3,
    longExplain: spec.longExplain ?? false,
    diagnosticStages: spec.diagnosticStages === undefined ? null : buildDiagnosticStages(spec),
    featureCatalog: buildFeatureCatalog(spec),
    seed: spec.seed,
    missingResources: spec.missingResources ?? [],
    openingDelayMs: spec.openingDelayMs ?? 0,
    metadataDelayMs: spec.metadataDelayMs ?? 0,
    viewDelayMs: spec.viewDelayMs ?? 0,
    ...(spec.openFailure ? { openFailure: spec.openFailure } : {}),
    ...(spec.failFirstOpen ? { failFirstOpen: spec.failFirstOpen } : {}),
    ...(spec.metadataFailure ? { metadataFailure: spec.metadataFailure } : {}),
    ...(spec.viewFailure ? { viewFailure: spec.viewFailure } : {}),
  };
}

export function buildGeometry(kind: DomainDescriptor['renderKind'], seed: number, irregular = false): RenderGeometry {
  if (kind === 'radial-profile') return radialGeometry(seed);
  const lonSteps = 96;
  const latSteps = 64;
  const positions: number[] = [];
  const normals: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  for (let y = 0; y <= latSteps; y += 1) {
    const v = y / latSteps;
    const theta = v * Math.PI;
    for (let x = 0; x <= lonSteps; x += 1) {
      const u = x / lonSteps;
      const phi = u * Math.PI * 2;
      const base: [number, number, number] = [Math.sin(theta) * Math.cos(phi), Math.cos(theta), Math.sin(theta) * Math.sin(phi)];
      const displacement = irregular ? 1 + Math.sin(base[0] * 7 + base[1] * 4 + seed) * 0.035 + Math.cos(base[2] * 5 - seed) * 0.022 : 1;
      positions.push(base[0] * displacement, base[1] * displacement, base[2] * displacement);
      const magnitude = Math.hypot(...base);
      normals.push(base[0] / magnitude, base[1] / magnitude, base[2] / magnitude);
      uvs.push(u, 1 - v);
    }
  }
  for (let y = 0; y < latSteps; y += 1) for (let x = 0; x < lonSteps; x += 1) {
    const a = y * (lonSteps + 1) + x;
    const b = a + lonSteps + 1;
    indices.push(a, b, a + 1, b, b + 1, a + 1);
  }
  return { kind, positions: Float32Array.from(positions), normals: Float32Array.from(normals), uvs: Float32Array.from(uvs), indices: Uint32Array.from(indices) };
}

function buildViewCatalog(domain: FixtureDomainSeed, spec: FixtureSpec, domainIndex: number): ViewCatalog {
  const groupCount = domain.groups;
  const groupLabels = domain.groupLabels ?? spec.groupLabels ?? [];
  const groups: ViewGroupDescriptor[] = Array.from({ length: groupCount }, (_, index) => ({
    id: `group-${domainIndex}-${index + 1}`,
    label: groupLabels[index] ?? (domain.longNames || spec.longNames ? `Capability group ${index + 1} with a descriptor supplied name that intentionally exceeds the compact menu width ${'with additional context '.repeat(2)}` : `Capability ${String(index + 1).padStart(2, '0')}`),
    order: index,
  }));
  const counts = domain.viewCounts ?? spec.viewCounts ?? Array.from({ length: groupCount }, () => spec.viewsPerGroup ?? 3);
  const views: ViewDescriptor[] = [];
  let globalIndex = domainIndex * 1000;
  for (const [groupIndex, group] of groups.entries()) {
    const count = counts[groupIndex] ?? 0;
    for (let index = 0; index < count; index += 1) {
      const ordinal = globalIndex++;
      const id = `${domain.id}-view-${String(ordinal).padStart(3, '0')}`;
      const baseLabel = viewLabel(group.label, index, spec.id, ordinal);
      const long = domain.longNames || spec.longNames;
      const label = long ? `${baseLabel} · descriptor label ${String(index + 1).padStart(2, '0')} ${'extended view metadata '.repeat(3)}` : baseLabel;
      const temporalIndex = spec.temporalSlices?.[index];
      const timeSelections = temporalIndex === undefined ? undefined : buildTimeSelections(temporalIndex, spec.longTemporalLabels ?? false);
      const isCategoryScenario = spec.categoryCount !== undefined;
      const category = isCategoryScenario && index === 0;
      const noStats = isCategoryScenario && index >= 1;
      const legend = category
        ? { kind: 'categorical' as const, ...(spec.includeUnit === false ? {} : { unit: 'fixture units' }), categories: buildCategories(spec) }
        : {
            kind: 'continuous' as const,
            ...(index === 1 || spec.includeUnit === false ? {} : { unit: 'display units' }),
            stops: [{ at: 0, color: '#55606a' }, { at: 0.5, color: '#a9b0b5' }, { at: 1, color: '#eef0ec' }],
            statsAvailable: spec.includeStats !== false && !noStats && !(spec.hugeRange && index === 1),
            ...(spec.hugeRange && index === 1 ? { range: { min: 1e-280, max: 9e278 } } : {}),
          };
      views.push({
        id,
        fieldOrDerivedId: `fixture-field/${id}`,
        operator: 'identity',
        domainId: domain.id,
        groupId: group.id,
        label,
        description: `${label} · synthetic display descriptor from ${spec.label}.`,
        legend,
        ...(timeSelections ? { timeSelections, defaultTimeSelectionId: timeSelections[0]?.id ?? 'mean' } : {}),
      });
    }
  }
  return { groups, views };
}

function viewLabel(group: string, index: number, scenarioId: string, ordinal: number): string {
  if (scenarioId === 'fixture:normal-surface') {
    const named: Record<string, readonly string[]> = {
      Spatial: ['Surface form', 'Reference grid', 'Domain extent', 'Mesh coverage'],
      Topography: ['Height', 'Roughness', 'Slope', 'Surface process'],
      Tectonics: ['Plate identity', 'Boundary distance', 'Crust type', 'Crust age'],
      Ocean: ['Coverage', 'Depth', 'Salinity', 'Current index'],
      Climate: ['Temperature', 'Precipitation', 'Pressure', 'Seasonal mean'],
    };
    return named[group]?.[index] ?? `${group} view ${index + 1}`;
  }
  if (scenarioId === 'fixture:radial') return ['Density', 'Temperature', 'Pressure', 'Hydrogen fraction'][index] ?? `Profile ${index + 1}`;
  if (scenarioId === 'fixture:irregular') return ['Shape', 'Radius', 'Material', 'Surface temperature'][index] ?? `View ${index + 1}`;
  return `${group} view ${String(ordinal + 1).padStart(2, '0')}`;
}

function buildTimeSelections(count: number, longLabels: boolean): ViewDescriptor['timeSelections'] {
  return [
    { id: 'mean', label: 'Mean' },
    ...Array.from({ length: count }, (_, index) => ({
      id: `slice-${index + 1}`,
      label: longLabels && index % 4 === 0
        ? `Selection ${String(index + 1).padStart(2, '0')} with an unusually long provider label ${'for overflow checking '.repeat(2)}`
        : `Slice ${String(index + 1).padStart(2, '0')}`,
    })),
  ];
}

function buildCategories(spec: FixtureSpec) {
  return Array.from({ length: spec.categoryCount ?? 0 }, (_, index) => {
    const rawCount = index === 0 ? 9_000_000_000_000 : index === 1 ? 1 : index % 7 === 0 ? 0 : Math.max(2, 35_000 - index * index * 19);
    return {
      label: spec.longCategoryLabels ? `Category ${String(index + 1).padStart(2, '0')} ${'provider label with extended descriptive text '.repeat(2)}` : `Category ${String(index + 1).padStart(2, '0')}`,
      color: `hsl(${(index * 47) % 360} 10% ${35 + (index % 4) * 8}%)`,
      count: spec.unevenCounts ? rawCount : index + 1,
      ...(spec.unevenCounts ? { weightedPercent: index === 0 ? 99.97 : index === 1 ? 0.001 : 0.03 / Math.max(1, (spec.categoryCount ?? 1) - 2) } : {}),
    };
  });
}

function buildPointReport(spec: FixtureSpec): PointReport {
  const count = spec.pointFields ?? 6;
  const groupCount = count === 0 ? 0 : Math.min(5, Math.ceil(count / 10));
  const groups = Array.from({ length: groupCount }, (_, groupIndex) => {
    const start = Math.floor(groupIndex * count / groupCount);
    const end = Math.floor((groupIndex + 1) * count / groupCount);
    return {
      id: `report-group-${groupIndex + 1}`,
      label: spec.longPointLabels ? `Provider grouping ${groupIndex + 1} ${'with a long descriptive label '.repeat(2)}` : `Capability ${String(groupIndex + 1).padStart(2, '0')}`,
      fields: Array.from({ length: end - start }, (_, offset) => {
        const index = start + offset;
        const noData = index % 13 === 4;
        const unitless = index % 4 === 1;
        const sources = ['stored', 'pyramid', 'inherited', 'refined', 'derived'];
        return {
          id: `field-${index + 1}`,
          label: spec.longPointLabels ? `Field ${String(index + 1).padStart(2, '0')} ${'provider field label '.repeat(3)}` : `Field ${String(index + 1).padStart(2, '0')}`,
          value: noData ? null : spec.longPointLabels && index === 0 ? `${'Synthetic value payload '.repeat(6)}${index + 1}` : `${(index * 17.23 - 36.4).toFixed(3)}`,
          ...(unitless ? {} : { unit: `unit-${index % 5 + 1}` }),
          ...(noData ? { nodata: true } : {}),
          sourceKind: sources[index % sources.length] ?? 'stored',
          levelUsed: index % 7 === 0 ? null : `L${String(index % 14).padStart(2, '0')}`,
        };
      }),
    };
  });
  return { positionLabel: 'Presentation-space pick', positionValue: '0.218, -0.447, 0.868', groups };
}

function buildDiagnosticStages(spec: FixtureSpec): readonly DiagnosticStage[] | null {
  if (spec.diagnosticStages === null) return null;
  return Array.from({ length: spec.diagnosticStages ?? 0 }, (_, index) => ({
    id: `stage-${index + 1}`,
    label: spec.longDiagnosticNames ? `Stage ${String(index + 1).padStart(2, '0')} ${'provider diagnostic stage name '.repeat(2)}` : `Stage ${String(index + 1).padStart(2, '0')}`,
  }));
}

function buildFeatureCatalog(spec: FixtureSpec): FeatureCatalog {
  const count = spec.featureTables ?? 0;
  return {
    tables: Array.from({ length: count }, (_, index) => ({
      id: `table-${index + 1}`,
      label: `Overlay table ${String(index + 1).padStart(2, '0')}${index % 5 === 0 ? ` ${'with extended descriptor supplied label '.repeat(2)}` : ''}`,
      count: index * 177,
      ...((spec.geometryEvery ?? 0) > 0 && index % (spec.geometryEvery ?? 1) === 0 ? { geometry: { color: '#c6cbd0', paths: [[[-0.65, 0.13, 0.7], [-0.12, 0.24, 0.94], [0.39, 0.18, 0.82]]] as const } } : {}),
    })),
  };
}

function radialGeometry(seed: number): RenderGeometry {
  const profile = Array.from({ length: 64 }, (_, index) => {
    const radius = index / 63;
    return Math.max(0.08, 0.65 + Math.sin(radius * 11 + seed) * 0.13 + Math.cos(radius * 4 + seed * 0.2) * 0.17);
  });
  const positions: number[] = [];
  const normals: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  const rings = 40;
  const segments = 120;
  for (let ring = 0; ring <= rings; ring += 1) {
    const radius = ring / rings;
    for (let segment = 0; segment <= segments; segment += 1) {
      const angle = (segment / segments) * Math.PI * 2;
      positions.push(Math.cos(angle) * radius, Math.sin(angle) * radius, 0);
      normals.push(0, 0, 1);
      uvs.push(0.5 + Math.cos(angle) * radius * 0.5, 0.5 + Math.sin(angle) * radius * 0.5);
    }
  }
  for (let ring = 0; ring < rings; ring += 1) for (let segment = 0; segment < segments; segment += 1) {
    const angle = ((segment + 0.5) / segments) * Math.PI * 2;
    if (angle > 4.15 && angle < 5.08) continue;
    const a = ring * (segments + 1) + segment;
    const b = a + segments + 1;
    indices.push(a, b, a + 1, b, b + 1, a + 1);
  }
  return { kind: 'radial-profile', positions: Float32Array.from(positions), normals: Float32Array.from(normals), uvs: Float32Array.from(uvs), indices: Uint32Array.from(indices), profile };
}

export function seededValue(seed: number, x: number, y: number, channel: number): number {
  const input = Math.imul(seed + 1, 0x45d9f3b) ^ Math.imul(x + 7, 0x27d4eb2d) ^ Math.imul(y + 13, 0x165667b1) ^ channel;
  let value = input >>> 0;
  value ^= value >>> 16;
  value = Math.imul(value, 0x7feb352d);
  value ^= value >>> 15;
  value = Math.imul(value, 0x846ca68b);
  value ^= value >>> 16;
  return (value >>> 0) / 4294967295;
}
