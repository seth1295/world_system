import type {
  BodyCatalog,
  BodyOption,
  BodyProvider,
  BodySummary,
  DiagnosticSummary,
  DisplayTile,
  FeatureOverlay,
  LegendDescriptor,
  PickPosition,
  PointReport,
  RadialProfilePoint,
  RenderGeometry,
  Vec3,
  ViewDescriptor,
  ViewStats,
} from './domain';

const palette = (...colors: string[]): LegendDescriptor['stops'] =>
  colors.map((color, index) => ({ at: index / (colors.length - 1), color }));

const spectrum = [
  { at: 0, color: '#112d49' },
  { at: 0.17, color: '#1e5268' },
  { at: 0.31, color: '#3d777b' },
  { at: 0.45, color: '#729483' },
  { at: 0.61, color: '#b1aa84' },
  { at: 0.78, color: '#d0bd96' },
  { at: 1, color: '#f1e7d2' },
];
const relief = palette('#183543', '#56818a', '#b2aa82', '#e9d6b0');
const tectonic = palette('#1f3150', '#4c70a1', '#d09567', '#e7c394');
const heat = palette('#192e55', '#586994', '#bf7b5f', '#f0c17b');
const stellar = palette('#332d61', '#8268a4', '#e58d72', '#ffe2a1');

function view(
  id: string,
  group: string,
  label: string,
  description: string,
  stops: LegendDescriptor['stops'],
  unit = '',
): ViewDescriptor {
  return {
    id,
    domain: id.startsWith('star-') ? 'interior' : 'surface',
    field: id,
    group,
    label,
    description,
    legend: { kind: 'continuous', unit, stops },
  };
}

const veyraViews: ViewDescriptor[] = [
  view('surface-form', 'Spatial', 'Surface form', 'A presentation map of the mock body surface.', spectrum),
  view('height', 'Topography', 'Height', 'Mock relief above a declared reference surface.', relief, 'km'),
  view('roughness', 'Topography', 'Roughness', 'A compact view of local surface texture.', palette('#172e41', '#477b7f', '#c5a677'), 'm'),
  view('crust', 'Tectonics', 'Crust type', 'Illustrative crust regions from the mock provider.', tectonic),
  view('plate', 'Tectonics', 'Plate identity', 'A categorical presentation of mock plate regions.', palette('#304d68', '#8a755d', '#b98267')),
  view('ocean', 'Ocean', 'Land / ocean', 'Mock water coverage around the declared level.', palette('#152e52', '#346276', '#a7a17a')),
  view('temperature', 'Climate', 'Temperature', 'A representative mean temperature layer.', heat, 'K'),
];

const starViews: ViewDescriptor[] = [
  view('star-density', 'Stellar structure', 'Density', 'Illustrative density through the stellar radius.', stellar, 'g/cm³'),
  view('star-temperature', 'Stellar structure', 'Temperature', 'Illustrative temperature through the stellar radius.', heat, 'MK'),
  view('star-pressure', 'Stellar structure', 'Pressure', 'Illustrative pressure through the stellar radius.', palette('#28365b', '#755f90', '#d08a73', '#f5d197'), 'GPa'),
  view('star-hydrogen', 'Stellar structure', 'Hydrogen fraction', 'Illustrative composition by radius.', palette('#463766', '#6a6690', '#d59773', '#f5dfad')),
];

const rockViews: ViewDescriptor[] = [
  view('rock-shape', 'Spatial', 'Shape', 'A display surface for the irregular mock figure.', palette('#424a54', '#77766d', '#aaa08b')),
  view('rock-radius', 'Solid surface', 'Radius', 'A relative-radius view of the mock figure.', palette('#3d454e', '#797a70', '#c1b094'), 'km'),
  view('rock-material', 'Surface material', 'Surface material', 'Illustrative surface material regions.', palette('#5d5151', '#a08065', '#d6b68d')),
  view('rock-temperature', 'Thermal state', 'Surface temperature', 'A mock instantaneous surface temperature layer.', heat, 'K'),
];

interface MockDefinition {
  summary: BodySummary;
  views: ViewDescriptor[];
  seed: number;
  profileAmplitude: number;
  stats: Record<string, ViewStats>;
  fields: PointReport['fields'];
  featurePaths: Vec3[][];
}

const definitions: MockDefinition[] = [
  {
    summary: {
      objectId: 'obj:9f3c2a7d',
      name: 'Veyra',
      marker: '◉',
      classification: 'Terrestrial · habitable test body',
      description: 'A broad, quiet view of an ocean-bearing rocky world.',
      domainLabel: 'Surface domain',
      lod: 'L09 · native',
      sourceLabel: 'Mock provider',
      presentation: 'surface',
      shape: 'sphere',
      defaultViewId: 'surface-form',
    },
    views: veyraViews,
    seed: 17,
    profileAmplitude: 0.25,
    stats: {
      'surface-form': { min: -0.8, max: 1.0, mean: 0.18, countLabel: '12.4M samples' },
      height: { min: -6.2, max: 8.4, mean: 0.3, countLabel: '8.2M cells' },
      roughness: { min: 14, max: 740, mean: 186, countLabel: '8.2M cells' },
      crust: { min: 0, max: 4, mean: 1.6, countLabel: '8.2M cells' },
      plate: { min: 0, max: 10, mean: 4.8, countLabel: '8.2M cells' },
      ocean: { min: 0, max: 1, mean: 0.67, countLabel: '8.2M cells' },
      temperature: { min: 181, max: 322, mean: 287, countLabel: '1.9M cells' },
    },
    fields: [
      { label: 'Height', value: '+2.41', unit: 'km', color: '#d8c99f' },
      { label: 'Roughness', value: '186', unit: 'm', color: '#7ea4a1' },
      { label: 'Plate', value: 'Plate 07', color: '#d8a875' },
      { label: 'Crust type', value: 'Continental', color: '#b89b7b' },
    ],
    featurePaths: [
      [[-0.71, 0.12, 0.68], [-0.42, 0.22, 0.87], [-0.08, 0.29, 0.96], [0.28, 0.34, 0.89], [0.63, 0.29, 0.72]],
      [[-0.46, -0.68, 0.57], [-0.22, -0.75, 0.62], [0.04, -0.71, 0.70], [0.32, -0.61, 0.73]],
    ],
  },
  {
    summary: {
      objectId: 'obj:6b18e441',
      name: 'Auren',
      marker: '✦',
      classification: 'Stellar · main sequence lite',
      description: 'A radial cutaway through a luminous stellar profile.',
      domainLabel: 'Interior · radial profile',
      lod: 'Profile · 64 shells',
      sourceLabel: 'Mock provider',
      presentation: 'radial-profile',
      shape: 'radial',
      defaultViewId: 'star-density',
    },
    views: starViews,
    seed: 31,
    profileAmplitude: 0.82,
    stats: {
      'star-density': { min: 0.01, max: 151, mean: 23.4, countLabel: '64 radial shells' },
      'star-temperature': { min: 0.01, max: 15.7, mean: 4.8, countLabel: '64 radial shells' },
      'star-pressure': { min: 0.02, max: 2200, mean: 418, countLabel: '64 radial shells' },
      'star-hydrogen': { min: 0.28, max: 0.71, mean: 0.54, countLabel: '64 radial shells' },
    },
    fields: [
      { label: 'Radius', value: '0.64', unit: 'R★', color: '#ecc28d' },
      { label: 'Density', value: '18.6', unit: 'g/cm³', color: '#c78786' },
      { label: 'Temperature', value: '5.2', unit: 'MK', color: '#f0bc7b' },
      { label: 'Hydrogen fraction', value: '0.58', color: '#b4a0ca' },
    ],
    featurePaths: [],
  },
  {
    summary: {
      objectId: 'obj:c8a0472d',
      name: 'Irregular Rock',
      marker: '◈',
      classification: 'Airless · irregular solid',
      description: 'A small, quiet body with an uneven star-convex figure.',
      domainLabel: 'Surface domain',
      lod: 'L06 · native',
      sourceLabel: 'Mock provider',
      presentation: 'surface',
      shape: 'irregular',
      defaultViewId: 'rock-shape',
    },
    views: rockViews,
    seed: 53,
    profileAmplitude: 0.44,
    stats: {
      'rock-shape': { min: -0.18, max: 0.21, mean: 0.02, countLabel: '1.1M surface samples' },
      'rock-radius': { min: 7.1, max: 9.7, mean: 8.2, countLabel: '1.1M surface samples' },
      'rock-material': { min: 0, max: 3, mean: 1.5, countLabel: '1.1M surface samples' },
      'rock-temperature': { min: 104, max: 318, mean: 217, countLabel: '1.1M surface samples' },
    },
    fields: [
      { label: 'Relative radius', value: '0.93', unit: '× mean', color: '#c1b094' },
      { label: 'Material', value: 'Silicate regolith', color: '#c29a78' },
      { label: 'Temperature', value: '217', unit: 'K', color: '#d29a79' },
      { label: 'Reference surface', value: 'Figure only', color: '#8e979c' },
    ],
    featurePaths: [],
  },
];

const options: BodyOption[] = definitions.map(({ summary }) => ({
  objectId: summary.objectId,
  label: summary.name,
  subtitle: summary.classification,
  marker: summary.marker,
}));

export class MockBodyCatalog implements BodyCatalog {
  async bodies(): Promise<readonly BodyOption[]> {
    return options;
  }

  async open(objectId: string): Promise<BodyProvider> {
    const definition = definitions.find((candidate) => candidate.summary.objectId === objectId);
    if (!definition) throw new Error(`Unknown mock body: ${objectId}`);
    return new MockBodyProvider(definition);
  }
}

export class MockBodyProvider implements BodyProvider {
  constructor(private readonly definition: MockDefinition) {}

  async summary(): Promise<BodySummary> {
    return this.definition.summary;
  }

  async views(): Promise<readonly ViewDescriptor[]> {
    return this.definition.views;
  }

  async stats(viewId: string): Promise<ViewStats> {
    const result = this.definition.stats[viewId];
    if (!result) throw new Error(`Unknown view: ${viewId}`);
    return result;
  }

  async domainGeometry(): Promise<RenderGeometry> {
    return this.definition.summary.shape === 'radial'
      ? radialGeometry(192)
      : surfaceGeometry(this.definition.summary.shape, 144, 96);
  }

  async tile(viewId: string): Promise<DisplayTile> {
    const descriptor = this.definition.views.find((candidate) => candidate.id === viewId);
    if (!descriptor) throw new Error(`Unknown view: ${viewId}`);

    const radial = this.definition.summary.presentation === 'radial-profile';
    const width = 512;
    const height = radial ? 256 : 256;
    const values = new Float32Array(width * height);
    const seed = this.definition.seed + viewId.length * 7;
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const u = x / (width - 1);
        const v = y / (height - 1);
        const radius = Math.sqrt((u - 0.5) ** 2 + (v - 0.5) ** 2) * 2;
        const value = radial
          ? stellarValue(radius, seed, this.definition.profileAmplitude)
          : surfaceValue(u, v, seed, this.definition.summary.shape, viewId);
        values[y * width + x] = Math.max(0, Math.min(1, value));
      }
    }
    return { width, height, values, minimum: 0, maximum: 1 };
  }

  async radialProfile(viewId: string): Promise<readonly RadialProfilePoint[]> {
    if (this.definition.summary.presentation !== 'radial-profile') return [];
    return Array.from({ length: 64 }, (_, index) => {
      const radius = index / 63;
      return { radius, value: stellarValue(radius, this.definition.seed + viewId.length * 7, this.definition.profileAmplitude) };
    });
  }

  async inspect(position: PickPosition): Promise<PointReport> {
    const positionLabel = position.kind === 'radial-distance'
      ? `Radius ${(position.normalizedRadius * 100).toFixed(1)}%`
      : `Surface ${formatDirection(position.direction)}`;
    return {
      positionLabel,
      fields: this.definition.fields,
      source: 'Mock provider · representative values',
      levelUsed: this.definition.summary.lod,
    };
  }

  async explain(_position: PickPosition, viewId: string): Promise<string[]> {
    const descriptor = this.definition.views.find((candidate) => candidate.id === viewId);
    return descriptor
      ? [
          `${descriptor.label} · illustrative mock layer`,
          'Values supplied by MockBodyProvider for visual prototyping.',
          'No canonical derivation or feature lookup is performed in the browser.',
        ]
      : [];
  }

  async features(): Promise<readonly FeatureOverlay[]> {
    if (this.definition.featurePaths.length === 0) return [];
    return [{ label: 'Plate boundaries · mock overlay', color: '#e6b685', paths: this.definition.featurePaths }];
  }

  async diagnostics(): Promise<DiagnosticSummary> {
    return { available: false };
  }
}

function surfaceGeometry(shape: 'sphere' | 'irregular', longitudeSteps: number, latitudeSteps: number): RenderGeometry {
  const positions: number[] = [];
  const normals: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  for (let latIndex = 0; latIndex <= latitudeSteps; latIndex += 1) {
    const v = latIndex / latitudeSteps;
    const theta = v * Math.PI;
    for (let lonIndex = 0; lonIndex <= longitudeSteps; lonIndex += 1) {
      const u = lonIndex / longitudeSteps;
      const phi = u * Math.PI * 2;
      const direction: Vec3 = [Math.sin(theta) * Math.cos(phi), Math.cos(theta), Math.sin(theta) * Math.sin(phi)];
      const wobble = shape === 'irregular'
        ? 1 + Math.sin(direction[0] * 7 + direction[1] * 4) * 0.075 + Math.cos(direction[2] * 8 - direction[0] * 3) * 0.045
        : 1 + Math.sin(direction[0] * 5 + direction[2] * 4) * 0.006 + Math.cos(direction[1] * 6) * 0.004;
      positions.push(direction[0] * wobble, direction[1] * wobble, direction[2] * wobble);
      const length = Math.hypot(...direction);
      normals.push(direction[0] / length, direction[1] / length, direction[2] / length);
      uvs.push(u, 1 - v);
    }
  }
  for (let latIndex = 0; latIndex < latitudeSteps; latIndex += 1) {
    for (let lonIndex = 0; lonIndex < longitudeSteps; lonIndex += 1) {
      const first = latIndex * (longitudeSteps + 1) + lonIndex;
      const second = first + longitudeSteps + 1;
      indices.push(first, first + 1, second, second, first + 1, second + 1);
    }
  }
  return {
    kind: 'surface',
    positions: Float32Array.from(positions),
    normals: Float32Array.from(normals),
    uvs: Float32Array.from(uvs),
    indices: Uint32Array.from(indices),
  };
}

function radialGeometry(steps: number): RenderGeometry {
  const positions = [0, 0, 0];
  const normals = [0, 0, 1];
  const uvs = [0.5, 0.5];
  const indices: number[] = [];
  for (let index = 0; index <= steps; index += 1) {
    const angle = (index / steps) * Math.PI * 2;
    const x = Math.cos(angle);
    const y = Math.sin(angle);
    positions.push(x, y, 0);
    normals.push(0, 0, 1);
    uvs.push(x * 0.5 + 0.5, y * 0.5 + 0.5);
  }
  for (let index = 1; index <= steps; index += 1) indices.push(0, index, index + 1);
  return {
    kind: 'radial-profile',
    positions: Float32Array.from(positions),
    normals: Float32Array.from(normals),
    uvs: Float32Array.from(uvs),
    indices: Uint32Array.from(indices),
  };
}

function surfaceValue(u: number, v: number, seed: number, shape: 'sphere' | 'irregular' | 'radial', viewId: string): number {
  const broad = Math.sin(u * 17 + seed) * Math.cos(v * 7 + seed * 0.37);
  const small = Math.sin(u * 61 - seed * 0.7 + Math.sin(v * 20)) * Math.cos(v * 39 + seed);
  if (shape === 'sphere') {
    if (viewId === 'surface-form') {
      const regions = [
        [0.08, 0.42, 0.075, 0.17], [0.24, 0.54, 0.115, 0.2],
        [0.48, 0.39, 0.095, 0.16], [0.68, 0.6, 0.125, 0.17],
        [0.88, 0.34, 0.082, 0.14], [0.38, 0.83, 0.11, 0.085],
      ];
      let coast = -1.2;
      for (const [centerU, centerV, width, height] of regions) {
        if (centerU === undefined || centerV === undefined || width === undefined || height === undefined) continue;
        const wrappedDistance = Math.min(Math.abs(u - centerU), 1 - Math.abs(u - centerU));
        const distance = Math.sqrt((wrappedDistance / width) ** 2 + ((v - centerV) / height) ** 2);
        coast = Math.max(coast, 1 - distance);
      }
      const edgeNoise = Math.sin(u * 34 + Math.cos(v * 17 + seed)) * 0.075
        + Math.cos(v * 39 + Math.sin(u * 25 - seed)) * 0.055
        + small * 0.14;
      coast += edgeNoise;
      const terrain = broad * 0.3 + small * 0.19 + Math.sin(u * 36 + Math.cos(v * 24)) * Math.cos(v * 33 + Math.sin(u * 17)) * 0.14;
      if (coast < -0.09) return Math.max(0.015, 0.045 + terrain * 0.035);
      if (coast < 0.12) return 0.16 + (coast + 0.09) * 0.92 + terrain * 0.025;
      return Math.max(0.35, Math.min(0.99, 0.45 + Math.max(0, coast) * 0.24 + terrain * 0.19));
    }
    const land = Math.sin(u * 8 + Math.cos(v * 8) * 1.4 + seed * 0.08) + Math.cos(v * 12 + Math.sin(u * 15)) * 0.8;
    const ridged = Math.abs(Math.sin(u * 25 + Math.cos(v * 13)) * Math.cos(v * 23 + Math.sin(u * 11)));
    return Math.max(0, Math.min(1, 0.43 + land * 0.12 + broad * 0.14 + small * 0.11 + ridged * 0.14));
  }
  return Math.max(0, Math.min(1, 0.47 + broad * 0.18 + small * 0.15));
}

function stellarValue(radius: number, seed: number, amplitude: number): number {
  const shell = Math.exp(-radius * (1.5 + amplitude)) * 0.7 + Math.sin(radius * 16 + seed) * 0.065 + Math.cos(radius * 30 + seed * 0.4) * 0.035;
  return Math.max(0, Math.min(1, shell));
}

function formatDirection(direction: Vec3): string {
  return `${direction[0].toFixed(2)}, ${direction[1].toFixed(2)}, ${direction[2].toFixed(2)}`;
}
