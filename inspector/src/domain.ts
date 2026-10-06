export type Vec3 = readonly [number, number, number];

export interface BodyOption {
  objectId: string;
  label: string;
  subtitle: string;
  marker: string;
}

export interface BodySummary {
  objectId: string;
  name: string;
  marker: string;
  classification: string;
  description: string;
  domainLabel: string;
  lod: string;
  sourceLabel: string;
  presentation: 'surface' | 'radial-profile';
  shape: 'sphere' | 'irregular' | 'radial';
  defaultViewId: string;
}

export type PaletteStop = { at: number; color: string };

export interface LegendDescriptor {
  kind: 'continuous' | 'categorical';
  unit?: string;
  stops: PaletteStop[];
  categories?: { label: string; color: string }[];
}

export interface ViewDescriptor {
  id: string;
  domain: string;
  field: string;
  group: string;
  label: string;
  description: string;
  legend: LegendDescriptor;
}

export interface ViewStats {
  min: number;
  max: number;
  mean: number;
  countLabel: string;
}

export interface RenderGeometry {
  kind: 'surface' | 'radial-profile';
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  indices: Uint32Array;
}

export interface DisplayTile {
  width: number;
  height: number;
  values: Float32Array;
  minimum: number;
  maximum: number;
}

export interface RadialProfilePoint {
  radius: number;
  value: number;
}

export type PickPosition =
  | { kind: 'surface-direction'; direction: Vec3 }
  | { kind: 'radial-distance'; normalizedRadius: number };

export interface PointField {
  label: string;
  value: string;
  unit?: string;
  color?: string;
}

export interface PointReport {
  positionLabel: string;
  fields: PointField[];
  source: string;
  levelUsed: string;
}

export interface FeatureOverlay {
  label: string;
  color: string;
  paths: Vec3[][];
}

export interface DiagnosticSummary {
  available: boolean;
  label?: string;
}

/** Consumer-facing body contract. A future adapter can implement this with veyra-wasm. */
export interface BodyProvider {
  summary(): Promise<BodySummary>;
  views(): Promise<readonly ViewDescriptor[]>;
  stats(viewId: string): Promise<ViewStats>;
  domainGeometry(): Promise<RenderGeometry>;
  tile(viewId: string): Promise<DisplayTile>;
  radialProfile(viewId: string): Promise<readonly RadialProfilePoint[]>;
  inspect(position: PickPosition): Promise<PointReport>;
  explain(position: PickPosition, viewId: string): Promise<string[]>;
  features(): Promise<readonly FeatureOverlay[]>;
  diagnostics(): Promise<DiagnosticSummary>;
}

/** Catalogue seam: page controls can discover and open bodies without mock constants. */
export interface BodyCatalog {
  bodies(): Promise<readonly BodyOption[]>;
  open(objectId: string): Promise<BodyProvider>;
}
