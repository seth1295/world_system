export interface RegressionControlHandle {
  failNext(operation: string, key: string): void;
  operationCount(operation: string, key: string): number;
  explanationCount(viewId: string, fixtureId?: string): number;
  inspectionCount(): number;
  inspectionPositionKey(index: number): string | undefined;
  normalizeColor(value: unknown): { css: string; hex: string; rgba: readonly [number, number, number, number] } | null;
  samplePalette(stops: readonly { at: number; color: string }[], value: number): { css: string; hex: string; rgba: readonly [number, number, number, number] };
  pickPoint(position: { kind: 'surface-direction'; direction: readonly [number, number, number] }): void;
  resolveExplain(viewId: string, label: string, fixtureId?: string): void;
  rejectExplain(viewId: string, message: string, fixtureId?: string): void;
  resolveInspection(index: number, label: string): void;
  rejectInspection(index: number, message: string): void;
  setDiagnosticStageDeferred(value: boolean): void;
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
