import '../src/styles.css';
import type { BodyCatalog, BodyProvider, FixtureOption } from '../src/provider/contracts';
import { MockBodyProvider } from '../src/provider/mock-provider';
import { InspectorApp } from '../src/ui/app';
import { buildFixture, SCENARIOS } from '../src/fixtures/scenarios';

declare global {
  interface Window {
    catalogHarness: { opened: string[] };
  }
}

const sourceFixture = buildFixture(SCENARIOS.find(({ id }) => id === 'fixture:normal-surface')!);
const alternateFixtures: readonly FixtureOption[] = [
  { id: 'body:alpha', label: 'Alpha body', description: 'First opaque catalog entry.' },
  { id: 'body:beta', label: 'Beta body', description: 'Second opaque catalog entry.' },
];
const emptyCatalog = new URL(window.location.href).searchParams.get('empty') === '1';
const options = emptyCatalog ? [] : alternateFixtures;
const opened: string[] = [];

class AlternateCatalog implements BodyCatalog {
  fixtures(): readonly FixtureOption[] {
    return options;
  }

  async open(fixtureId: string): Promise<BodyProvider> {
    opened.push(fixtureId);
    if (!options.some(({ id }) => id === fixtureId)) throw new Error(`Unexpected fixture ID: ${fixtureId}`);
    return new MockBodyProvider(sourceFixture);
  }
}

const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Catalog harness root is missing');
window.catalogHarness = { opened };
const app = new InspectorApp(root, new AlternateCatalog());
window.addEventListener('pagehide', () => app.destroy(), { once: true });
void app.start();
