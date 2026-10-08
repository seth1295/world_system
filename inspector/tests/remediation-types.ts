import type { NormalizedColor } from '../src/render/css-color';

export interface OverlayMaterialState {
  color: string;
  opacity: number;
  transparent: boolean;
}

export interface RegressionControlHandle {
  failNext(operation: string, key: string): void;
  operationCount(operation: string, key: string): number;
  statsCount(viewId: string): number;
  setStatsDeferred(value: boolean): void;
  resolveStats(viewId: string, min: number, max: number, mean: number): void;
  rejectStats(viewId: string, message: string): void;
  setTilesDeferred(value: boolean): void;
  resolveTile(viewId: string): void;
  rejectTile(viewId: string, message: string): void;
  setViewsDeferred(value: boolean): void;
  resolveViews(domainId: string): void;
  rejectViews(domainId: string, message: string): void;
  resolveOpen(fixtureId: string): void;
  rejectOpen(fixtureId: string, message: string): void;
  destroyApp(): void;
  postDestroyViewportCalls(): number;
  postDestroyRenderCalls(): number;
  unhandledRejectionCount(): number;
  explanationCount(viewId: string, fixtureId?: string): number;
  inspectionCount(): number;
  inspectionPositionKey(index: number): string | undefined;
  normalizeColor(value: unknown): { css: string; hex: string; rgba: readonly [number, number, number, number] } | null;
  normalizeOverlayColor(value: unknown): NormalizedColor | null;
  overlayMaterialStates(): readonly OverlayMaterialState[];
  samplePalette(stops: readonly { at: number; color: string }[], value: number): { css: string; hex: string; rgba: readonly [number, number, number, number] };
  pickPoint(position: { kind: 'surface-direction'; direction: readonly [number, number, number] }): void;
  resolveExplain(viewId: string, label: string, fixtureId?: string): void;
  rejectExplain(viewId: string, message: string, fixtureId?: string): void;
  resolveInspection(index: number, label: string): void;
  rejectInspection(index: number, message: string): void;
  setDiagnosticStageDeferred(value: boolean): void;
  setLargeProfileSamples(count: number): void;
  diagnosticStageCount(stageId: string): number;
  resolveStage(stageId: string, label: string): void;
  rejectStage(stageId: string, message: string): void;
}

declare global {
  interface Window {
    remediationControl: RegressionControlHandle;
    hostileColorExecuted: boolean;
  }
}
