import * as THREE from 'three';
import { DEFAULT_CATEGORICAL_COLOR, normalizeCssColor, normalizePaletteStops, sampleNormalizedPalette } from './css-color';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type { DisplayTile, FeatureTable, PickPosition, RenderGeometry, ViewDescriptor } from '../provider/contracts';

type PickHandler = (position: PickPosition) => void;

export class Viewport {
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(34, 1, 0.1, 80);
  private readonly renderer: THREE.WebGLRenderer;
  private readonly controls: OrbitControls;
  private readonly root = new THREE.Group();
  private readonly raycaster = new THREE.Raycaster();
  private readonly pointer = new THREE.Vector2();
  private readonly pickables: THREE.Mesh[] = [];
  private readonly overlayObjects: THREE.Object3D[] = [];
  private readonly observer: ResizeObserver;
  private currentKind: RenderGeometry['kind'] | null = null;
  private fitDistance = 4.55;
  private pointerStart: { x: number; y: number } | null = null;
  private animationFrame = 0;
  private readonly onPick: PickHandler;

  constructor(private readonly host: HTMLDivElement, onPick: PickHandler) {
    this.onPick = onPick;
    this.scene.background = new THREE.Color('#111417');
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.NoToneMapping;
    this.renderer.setClearColor(0x111417, 1);
    this.renderer.domElement.className = 'viewport-canvas';
    this.renderer.domElement.setAttribute('aria-label', 'Interactive provider-supplied model. Drag to rotate and scroll to zoom.');
    this.renderer.domElement.dataset.testid = 'viewport-canvas';
    this.renderer.domElement.style.touchAction = 'none';
    this.host.append(this.renderer.domElement);
    this.scene.add(this.root);
    this.scene.add(new THREE.AmbientLight(0xb8bec5, 0.44));
    const key = new THREE.DirectionalLight(0xe2e5e8, 2.05);
    key.position.set(-3, 2.3, 4.5);
    this.scene.add(key);
    this.camera.position.set(0, 0, 3.65);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.target.set(0, 0, 0);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.06;
    this.controls.enablePan = false;
    this.controls.minDistance = 1.65;
    this.controls.maxDistance = 8;
    this.controls.addEventListener('change', this.render);
    this.renderer.domElement.addEventListener('pointerdown', this.pointerDown);
    this.renderer.domElement.addEventListener('pointerup', this.pointerUp);
    this.renderer.domElement.addEventListener('pointercancel', this.pointerCancel);
    this.observer = new ResizeObserver(() => this.resize());
    this.observer.observe(this.host);
    this.resize();
  }

  setData(geometry: RenderGeometry, descriptor: ViewDescriptor, tile: DisplayTile): void {
    this.clearRoot();
    this.currentKind = geometry.kind;
    const meshGeometry = new THREE.BufferGeometry();
    meshGeometry.setAttribute('position', new THREE.BufferAttribute(geometry.positions, 3));
    meshGeometry.setAttribute('normal', new THREE.BufferAttribute(geometry.normals, 3));
    meshGeometry.setAttribute('uv', new THREE.BufferAttribute(geometry.uvs, 2));
    meshGeometry.setIndex(new THREE.BufferAttribute(geometry.indices, 1));
    const texture = this.createTexture(tile, descriptor);
    const material = geometry.kind === 'radial-profile'
      ? new THREE.MeshBasicMaterial({ map: texture, side: THREE.DoubleSide })
      : new THREE.MeshStandardMaterial({ map: texture, roughness: 0.88, metalness: 0, side: THREE.DoubleSide });
    const model = new THREE.Mesh(meshGeometry, material);
    model.name = 'provider-geometry';
    model.scale.setScalar(1.25);
    this.root.add(model);
    this.pickables.push(model);
    if (geometry.kind === 'radial-profile') this.addProfileGuides();
    this.controls.enableRotate = geometry.kind === 'surface-mesh';
    this.controls.target.set(0, 0, 0);
    this.fitDistance = this.distanceForAspect(this.camera.aspect);
    this.camera.position.set(0, 0, this.fitDistance);
    this.controls.update();
    this.render();
  }

  clearData(): void {
    this.clearRoot();
    this.currentKind = null;
    this.render();
  }

  setOverlays(tables: readonly FeatureTable[]): void {
    for (const object of this.overlayObjects.splice(0)) {
      this.root.remove(object);
      disposeObject(object);
    }
    for (const table of tables) {
      for (const path of table.geometry?.paths ?? []) {
        const points = path.map((point) => new THREE.Vector3(point[0], point[1], point[2]).multiplyScalar(1.26));
        const geometry = new THREE.BufferGeometry().setFromPoints(points);
        const color = normalizeCssColor(table.geometry?.color);
        const line = new THREE.Line(geometry, new THREE.LineBasicMaterial({
          color: color?.hex ?? '#dedede',
          transparent: true,
          opacity: 0.85 * ((color?.rgba[3] ?? 255) / 255),
        }));
        this.root.add(line);
        this.overlayObjects.push(line);
      }
    }
    this.render();
  }

  inspectCenter(): void {
    if (this.currentKind === 'radial-profile') {
      this.onPick({ kind: 'radial-distance', normalizedRadius: 0 });
      return;
    }
    this.pickAt(new THREE.Vector2(0, 0));
  }

  destroy(): void {
    cancelAnimationFrame(this.animationFrame);
    this.observer.disconnect();
    this.controls.dispose();
    this.renderer.domElement.removeEventListener('pointerdown', this.pointerDown);
    this.renderer.domElement.removeEventListener('pointerup', this.pointerUp);
    this.renderer.domElement.removeEventListener('pointercancel', this.pointerCancel);
    for (const child of [...this.scene.children]) disposeObject(child);
    this.renderer.dispose();
    this.renderer.domElement.remove();
  }

  private createTexture(tile: DisplayTile, descriptor: ViewDescriptor): THREE.CanvasTexture {
    const canvas = document.createElement('canvas');
    canvas.width = tile.width;
    canvas.height = tile.height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('The browser could not create a presentation texture.');
    const pixels = context.createImageData(tile.width, tile.height);
    const continuousPalette = descriptor.legend.kind === 'continuous'
      ? normalizePaletteStops(descriptor.legend.stops)
      : null;
    const categoryColors = descriptor.legend.kind === 'categorical'
      ? descriptor.legend.categories.map(({ color }) => normalizeCssColor(color) ?? DEFAULT_CATEGORICAL_COLOR)
      : null;
    for (let index = 0; index < tile.values.length; index += 1) {
      const value = tile.values[index] ?? 0;
      const color = descriptor.legend.kind === 'categorical'
        ? categoryColors?.[Math.min(categoryColors.length - 1, Math.floor(value * categoryColors.length))] ?? DEFAULT_CATEGORICAL_COLOR
        : sampleNormalizedPalette(continuousPalette ?? [], value);
      pixels.data[index * 4] = color.rgba[0];
      pixels.data[index * 4 + 1] = color.rgba[1];
      pixels.data[index * 4 + 2] = color.rgba[2];
      pixels.data[index * 4 + 3] = color.rgba[3];
    }
    context.putImageData(pixels, 0, 0);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.ClampToEdgeWrapping;
    texture.minFilter = THREE.LinearMipmapLinearFilter;
    texture.magFilter = THREE.LinearFilter;
    texture.generateMipmaps = true;
    return texture;
  }

  private addProfileGuides(): void {
    for (const radius of [0.32, 0.65, 0.98]) {
      for (const [start, end] of [[0, 4.15], [5.08, Math.PI * 2]] as const) {
        const points = Array.from({ length: 48 }, (_, index) => {
          const angle = start + (end - start) * index / 47;
          return new THREE.Vector3(Math.cos(angle) * radius * 1.25, Math.sin(angle) * radius * 1.25, 0.006);
        });
        const geometry = new THREE.BufferGeometry().setFromPoints(points);
        const guide = new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: '#30373c', transparent: true, opacity: 0.78 }));
        this.root.add(guide);
      }
    }
  }

  private clearRoot(): void {
    for (const child of [...this.root.children]) {
      this.root.remove(child);
      disposeObject(child);
    }
    this.pickables.length = 0;
    this.overlayObjects.length = 0;
  }

  private resize(): void {
    const width = Math.max(1, this.host.clientWidth);
    const height = Math.max(1, this.host.clientHeight);
    const zoomRatio = this.camera.position.distanceTo(this.controls.target) / this.fitDistance;
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(width, height, false);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    if (this.currentKind) {
      this.fitDistance = this.distanceForAspect(this.camera.aspect);
      this.camera.position.setLength(this.fitDistance * zoomRatio);
      this.controls.update();
    }
    this.render();
  }

  private distanceForAspect(aspect: number): number {
    return 4.55 * Math.max(1, 1 / aspect);
  }

  private render = (): void => {
    if (this.animationFrame) return;
    this.animationFrame = requestAnimationFrame(() => {
      this.animationFrame = 0;
      this.controls.update();
      this.renderer.render(this.scene, this.camera);
    });
  };

  private pointerDown = (event: PointerEvent): void => {
    this.pointerStart = { x: event.clientX, y: event.clientY };
  };

  private pointerCancel = (): void => {
    this.pointerStart = null;
  };

  private pointerUp = (event: PointerEvent): void => {
    const start = this.pointerStart;
    this.pointerStart = null;
    if (!start || Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5) return;
    const bounds = this.renderer.domElement.getBoundingClientRect();
    this.pointer.set(((event.clientX - bounds.left) / bounds.width) * 2 - 1, -((event.clientY - bounds.top) / bounds.height) * 2 + 1);
    this.pickAt(this.pointer);
  };

  private pickAt(pointer: THREE.Vector2): void {
    if (!this.currentKind) return;
    this.scene.updateMatrixWorld(true);
    this.camera.updateMatrixWorld();
    this.raycaster.setFromCamera(pointer, this.camera);
    const hit = this.raycaster.intersectObjects(this.pickables, false)[0];
    if (!hit) return;
    const bodyPoint = hit.object.worldToLocal(hit.point.clone());
    if (this.currentKind === 'radial-profile') {
      const normalizedRadius = Math.min(1, Math.hypot(bodyPoint.x, bodyPoint.y));
      if (Number.isFinite(normalizedRadius)) this.onPick({ kind: 'radial-distance', normalizedRadius });
      return;
    }
    if (bodyPoint.lengthSq() === 0 || !Number.isFinite(bodyPoint.lengthSq())) return;
    const direction = bodyPoint.normalize();
    this.onPick({ kind: 'surface-direction', direction: [direction.x, direction.y, direction.z] });
  }
}

function disposeObject(object: THREE.Object3D): void {
  object.traverse((child) => {
    if (child instanceof THREE.Mesh || child instanceof THREE.Line) {
      child.geometry.dispose();
      const materials = Array.isArray(child.material) ? child.material : [child.material];
      for (const material of materials) {
        if ('map' in material && material.map instanceof THREE.Texture) material.map.dispose();
        material.dispose();
      }
    }
  });
}
