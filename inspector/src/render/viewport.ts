import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type {
  BodySummary,
  DisplayTile,
  FeatureOverlay,
  PickPosition,
  RenderGeometry,
  Vec3,
  ViewDescriptor,
} from '../domain';

type PickHandler = (position: PickPosition) => void;

export class Viewport {
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(37, 1, 0.1, 120);
  private readonly renderer: THREE.WebGLRenderer;
  private readonly controls: OrbitControls;
  private readonly bodyRoot = new THREE.Group();
  private readonly raycaster = new THREE.Raycaster();
  private readonly pointer = new THREE.Vector2();
  private readonly pickables: THREE.Object3D[] = [];
  private readonly featureObjects: THREE.Object3D[] = [];
  private readonly onPick: PickHandler;
  private pointerStart: { x: number; y: number } | null = null;
  private currentSummary: BodySummary | null = null;
  private resizeObserver: ResizeObserver;
  private frame = 0;
  private lastTime = 0;

  constructor(private readonly host: HTMLDivElement, onPick: PickHandler) {
    this.onPick = onPick;
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 1.16;
    this.renderer.setClearColor(0x080b11, 1);
    this.renderer.domElement.className = 'viewport-canvas';
    this.renderer.domElement.setAttribute('aria-label', 'Interactive three dimensional body model');
    this.renderer.domElement.dataset.testid = 'viewport-canvas';
    this.renderer.domElement.style.touchAction = 'none';
    this.host.append(this.renderer.domElement);

    this.camera.position.set(0.12, 0.03, 3.7);
    this.scene.background = new THREE.Color(0x080b11);
    this.scene.add(this.bodyRoot);
    this.addStars();
    this.addLighting();
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.target.set(-0.18, 0, 0);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.055;
    this.controls.enablePan = false;
    this.controls.minDistance = 1.55;
    this.controls.maxDistance = 8.2;
    this.controls.rotateSpeed = 0.52;
    this.controls.zoomSpeed = 0.72;
    this.controls.autoRotate = false;
    this.controls.autoRotateSpeed = 0.35;
    this.controls.addEventListener('change', () => this.requestRender());

    this.renderer.domElement.addEventListener('pointerdown', this.onPointerDown);
    this.renderer.domElement.addEventListener('pointerup', this.onPointerUp);
    this.renderer.domElement.addEventListener('pointercancel', this.onPointerCancel);
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(this.host);
    this.resize();
    this.requestRender();
  }

  setBody(summary: BodySummary, geometry: RenderGeometry, descriptor: ViewDescriptor, tile: DisplayTile): void {
    this.currentSummary = summary;
    this.clearBody();
    const buffer = new THREE.BufferGeometry();
    buffer.setAttribute('position', new THREE.BufferAttribute(geometry.positions, 3));
    buffer.setAttribute('normal', new THREE.BufferAttribute(geometry.normals, 3));
    buffer.setAttribute('uv', new THREE.BufferAttribute(geometry.uvs, 2));
    buffer.setIndex(new THREE.BufferAttribute(geometry.indices, 1));

    const texture = this.makeTexture(tile, descriptor);
    const isRadial = geometry.kind === 'radial-profile';
    const material = isRadial
      ? new THREE.MeshBasicMaterial({ map: texture, side: THREE.DoubleSide, toneMapped: false })
      : new THREE.MeshStandardMaterial({
          map: texture,
          roughness: summary.shape === 'irregular' ? 0.96 : 0.86,
          metalness: summary.shape === 'irregular' ? 0.08 : 0.01,
        });
    const body = new THREE.Mesh(buffer, material);
    body.name = 'provider-domain-geometry';
    body.castShadow = false;
    body.receiveShadow = true;
    this.bodyRoot.position.set(-0.18, -0.01, 0);
    this.bodyRoot.add(body);
    this.pickables.push(body);

    if (summary.presentation === 'surface' && summary.shape === 'sphere') this.addAtmosphere();
    if (summary.presentation === 'radial-profile') this.addRadialDetails();
    if (this.controls) {
      this.controls.enableRotate = summary.presentation === 'surface';
      this.controls.target.set(-0.18, 0, 0);
      const cameraDistance = summary.presentation === 'radial-profile' ? 3.95 : summary.shape === 'irregular' ? 4.15 : 3.7;
      this.camera.position.set(0.12, summary.presentation === 'radial-profile' ? 0 : 0.04, cameraDistance);
      this.controls.update();
    }
    this.requestRender();
  }

  setFeatures(features: readonly FeatureOverlay[]): void {
    for (const object of this.featureObjects.splice(0)) {
      this.bodyRoot.remove(object);
      disposeObject(object);
    }
    for (const feature of features) {
      for (const path of feature.paths) {
        const points = path.map((point) => new THREE.Vector3(point[0], point[1], point[2]).multiplyScalar(1.012));
        const geometry = new THREE.BufferGeometry().setFromPoints(points);
        const line = new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: feature.color, transparent: true, opacity: 0.88 }));
        this.bodyRoot.add(line);
        this.featureObjects.push(line);
      }
    }
    this.requestRender();
  }

  setAutoRotate(enabled: boolean): void {
    this.controls.autoRotate = enabled;
    this.requestRender();
  }

  destroy(): void {
    cancelAnimationFrame(this.frame);
    this.resizeObserver.disconnect();
    this.controls.dispose();
    this.renderer.domElement.removeEventListener('pointerdown', this.onPointerDown);
    this.renderer.domElement.removeEventListener('pointerup', this.onPointerUp);
    this.renderer.domElement.removeEventListener('pointercancel', this.onPointerCancel);
    for (const child of [...this.scene.children]) disposeObject(child);
    this.renderer.dispose();
    this.renderer.domElement.remove();
  }

  private makeTexture(tile: DisplayTile, descriptor: ViewDescriptor): THREE.CanvasTexture {
    const canvas = document.createElement('canvas');
    canvas.width = tile.width;
    canvas.height = tile.height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('The browser could not create a display texture.');
    const image = context.createImageData(tile.width, tile.height);
    const stops = descriptor.legend.stops.length > 0 ? descriptor.legend.stops : [{ at: 0, color: '#233447' }, { at: 1, color: '#d3bd91' }];
    for (let index = 0; index < tile.values.length; index += 1) {
      const color = samplePalette(stops, tile.values[index] ?? 0);
      image.data[index * 4] = color[0];
      image.data[index * 4 + 1] = color[1];
      image.data[index * 4 + 2] = color[2];
      image.data[index * 4 + 3] = 255;
    }
    context.putImageData(image, 0, 0);
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.wrapS = THREE.RepeatWrapping;
    texture.wrapT = THREE.ClampToEdgeWrapping;
    texture.anisotropy = Math.min(8, this.renderer.capabilities.getMaxAnisotropy());
    texture.minFilter = THREE.LinearMipmapLinearFilter;
    texture.magFilter = THREE.LinearFilter;
    texture.generateMipmaps = true;
    return texture;
  }

  private addAtmosphere(): void {
    const material = new THREE.ShaderMaterial({
      side: THREE.BackSide,
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      vertexShader: `
        varying vec3 vNormal;
        varying vec3 vViewPosition;
        void main() {
          vec4 viewPosition = modelViewMatrix * vec4(position, 1.0);
          vNormal = normalize(normalMatrix * normal);
          vViewPosition = -viewPosition.xyz;
          gl_Position = projectionMatrix * viewPosition;
        }`,
      fragmentShader: `
        varying vec3 vNormal;
        varying vec3 vViewPosition;
        void main() {
          float rim = pow(max(0.0, 0.78 - dot(normalize(vNormal), normalize(vViewPosition))), 2.2);
          gl_FragColor = vec4(0.32, 0.63, 0.78, rim * 0.38);
        }`,
    });
    const shell = new THREE.Mesh(new THREE.SphereGeometry(1.055, 80, 56), material);
    shell.name = 'subtle-atmosphere';
    this.bodyRoot.add(shell);
  }

  private addRadialDetails(): void {
    const ringColor = new THREE.Color('#f0c9a5');
    for (const radius of [0.28, 0.52, 0.76, 0.985]) {
      const geometry = new THREE.RingGeometry(radius, radius + 0.0045, 192);
      const material = new THREE.MeshBasicMaterial({ color: ringColor, transparent: true, opacity: radius > 0.97 ? 0.54 : 0.18, side: THREE.DoubleSide, depthWrite: false });
      const ring = new THREE.Mesh(geometry, material);
      ring.position.z = 0.012;
      this.bodyRoot.add(ring);
    }
    const wedge = new THREE.Shape();
    wedge.moveTo(0, 0);
    wedge.lineTo(Math.cos(-0.52), Math.sin(-0.52));
    wedge.absarc(0, 0, 1.002, -0.52, 0.23, false);
    wedge.lineTo(0, 0);
    const cut = new THREE.Mesh(
      new THREE.ShapeGeometry(wedge, 32),
      new THREE.MeshBasicMaterial({ color: 0x080b11, transparent: true, opacity: 0.84, side: THREE.DoubleSide, depthWrite: false }),
    );
    cut.position.z = 0.019;
    this.bodyRoot.add(cut);
    const edgePoints = [
      new THREE.Vector3(0, 0, 0.026),
      new THREE.Vector3(Math.cos(-0.52), Math.sin(-0.52), 0.026),
    ];
    const edge = new THREE.Line(
      new THREE.BufferGeometry().setFromPoints(edgePoints),
      new THREE.LineBasicMaterial({ color: 0xf2b77f, transparent: true, opacity: 0.74 }),
    );
    this.bodyRoot.add(edge);
    const arcPoints: THREE.Vector3[] = [];
    for (let index = 0; index <= 48; index += 1) {
      const angle = -0.52 + (index / 48) * 0.75;
      arcPoints.push(new THREE.Vector3(Math.cos(angle), Math.sin(angle), 0.026));
    }
    this.bodyRoot.add(new THREE.Line(
      new THREE.BufferGeometry().setFromPoints(arcPoints),
      new THREE.LineBasicMaterial({ color: 0xf2b77f, transparent: true, opacity: 0.74 }),
    ));
  }

  private addLighting(): void {
    const hemisphere = new THREE.HemisphereLight(0x9cafc7, 0x10131b, 1.12);
    this.scene.add(hemisphere);
    const key = new THREE.DirectionalLight(0xffefd4, 2.65);
    key.position.set(-3.4, 2.6, 4.5);
    this.scene.add(key);
    const fill = new THREE.DirectionalLight(0x7399bd, 0.54);
    fill.position.set(4, -1.7, 2.2);
    this.scene.add(fill);
    const rim = new THREE.DirectionalLight(0x496986, 0.68);
    rim.position.set(1.5, 1.2, -4.5);
    this.scene.add(rim);
  }

  private addStars(): void {
    const count = 1900;
    const positions = new Float32Array(count * 3);
    let state = 917;
    const random = (): number => {
      state = (state * 1664525 + 1013904223) >>> 0;
      return state / 4294967296;
    };
    for (let index = 0; index < count; index += 1) {
      const theta = random() * Math.PI * 2;
      const z = random() * 2 - 1;
      const radial = Math.sqrt(1 - z * z);
      const distance = 18 + random() * 46;
      positions[index * 3] = Math.cos(theta) * radial * distance;
      positions[index * 3 + 1] = z * distance;
      positions[index * 3 + 2] = Math.sin(theta) * radial * distance;
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    const material = new THREE.PointsMaterial({ color: 0xbac6d7, size: 0.055, sizeAttenuation: true, transparent: true, opacity: 0.68, depthWrite: false, toneMapped: false });
    const stars = new THREE.Points(geometry, material);
    stars.frustumCulled = false;
    this.scene.add(stars);
  }

  private clearBody(): void {
    for (const child of [...this.bodyRoot.children]) {
      this.bodyRoot.remove(child);
      disposeObject(child);
    }
    this.pickables.length = 0;
    this.featureObjects.length = 0;
  }

  private resize(): void {
    const width = Math.max(1, this.host.clientWidth);
    const height = Math.max(1, this.host.clientHeight);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(width, height, false);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.requestRender();
  }

  private requestRender = (): void => {
    if (this.frame !== 0) return;
    this.frame = requestAnimationFrame((time) => {
      this.frame = 0;
      const delta = this.lastTime === 0 ? 0 : Math.min(0.05, Math.max(0, (time - this.lastTime) / 1000));
      this.lastTime = time;
      this.controls.update(delta);
      this.renderer.render(this.scene, this.camera);
    });
  };

  private onPointerDown = (event: PointerEvent): void => {
    this.pointerStart = { x: event.clientX, y: event.clientY };
  };

  private onPointerCancel = (): void => {
    this.pointerStart = null;
  };

  private onPointerUp = (event: PointerEvent): void => {
    if (!this.pointerStart || !this.currentSummary) return;
    const moved = Math.hypot(event.clientX - this.pointerStart.x, event.clientY - this.pointerStart.y);
    this.pointerStart = null;
    if (moved > 5) return;
    const rect = this.renderer.domElement.getBoundingClientRect();
    this.pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    this.pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    this.raycaster.setFromCamera(this.pointer, this.camera);
    const intersections = this.raycaster.intersectObjects(this.pickables, false);
    const hit = intersections[0];
    if (!hit) return;
    const localPoint = this.bodyRoot.worldToLocal(hit.point.clone());
    if (this.currentSummary.presentation === 'radial-profile') {
      this.onPick({ kind: 'radial-distance', normalizedRadius: Math.max(0, Math.min(1, Math.hypot(localPoint.x, localPoint.y))) });
      return;
    }
    const direction = localPoint.normalize();
    const tuple: Vec3 = [direction.x, direction.y, direction.z];
    this.onPick({ kind: 'surface-direction', direction: tuple });
  };
}

function samplePalette(stops: readonly { at: number; color: string }[], value: number): [number, number, number] {
  const position = Math.max(0, Math.min(1, value));
  let rightIndex = stops.findIndex((stop) => stop.at >= position);
  if (rightIndex < 0) rightIndex = stops.length - 1;
  const right = stops[rightIndex] ?? { at: 1, color: '#d3bd91' };
  const left = stops[Math.max(0, rightIndex - 1)] ?? right;
  const span = Math.max(0.000001, right.at - left.at);
  const amount = rightIndex === 0 ? 0 : (position - left.at) / span;
  const leftColor = parseHex(left.color);
  const rightColor = parseHex(right.color);
  return [
    Math.round(leftColor[0] + (rightColor[0] - leftColor[0]) * amount),
    Math.round(leftColor[1] + (rightColor[1] - leftColor[1]) * amount),
    Math.round(leftColor[2] + (rightColor[2] - leftColor[2]) * amount),
  ];
}

function parseHex(value: string): [number, number, number] {
  const normalized = value.startsWith('#') ? value.slice(1) : value;
  const expanded = normalized.length === 3 ? normalized.split('').map((part) => part + part).join('') : normalized;
  const parsed = Number.parseInt(expanded.slice(0, 6), 16);
  return [(parsed >> 16) & 255, (parsed >> 8) & 255, parsed & 255];
}

function disposeObject(object: THREE.Object3D): void {
  object.traverse((child) => {
    if (child instanceof THREE.Mesh || child instanceof THREE.Line || child instanceof THREE.Points) {
      child.geometry.dispose();
      const materials = Array.isArray(child.material) ? child.material : [child.material];
      for (const material of materials) {
        for (const value of Object.values(material)) {
          if (value instanceof THREE.Texture) value.dispose();
        }
        material.dispose();
      }
    }
  });
}
