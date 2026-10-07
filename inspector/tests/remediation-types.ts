export interface RegressionControlHandle {
  explanationCount(viewId: string, fixtureId?: string): number;
  inspectionCount(): number;
  inspectionPositionKey(index: number): string | undefined;
  pickPoint(position: { kind: 'surface-direction'; direction: readonly [number, number, number] }): void;
  resolveExplain(viewId: string, label: string, fixtureId?: string): void;
  rejectExplain(viewId: string, message: string, fixtureId?: string): void;
  resolveInspection(index: number, label: string): void;
  rejectInspection(index: number, message: string): void;
}

declare global {
  interface Window {
    remediationControl: RegressionControlHandle;
    hostileColorExecuted: boolean;
  }
}
