import './styles.css';
import { InspectorApp } from './ui/app';
import { MockBodyCatalog } from './mock-provider';

const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Inspector app root is missing');

const app = new InspectorApp(root, new MockBodyCatalog());
void app.start();
