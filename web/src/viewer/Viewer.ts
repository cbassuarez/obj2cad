// 3D preview in CAD space (Z up). Display only: float32 buffers re-centered on the model's
// bounds; the exported file is produced separately from the exact double-precision data.
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { CSS2DObject, CSS2DRenderer } from "three/addons/renderers/CSS2DRenderer.js";
import type { PreviewBuffers } from "@/lib/engine";
import type { UpAxis } from "@/lib/settings";

export interface ViewerTheme {
  grid: string;
  gridMajor: string;
  dim: string;
  edge: string;
}

/** View-space direction of each world axis (x right, y up), for the axis gizmo. */
export type AxisDirs = { x: [number, number, number]; y: [number, number, number]; z: [number, number, number] };

export type ViewName = "iso" | "top" | "front" | "right";

/** Camera directions (from target to camera), as AutoCAD names them. */
const VIEWS: Record<ViewName, THREE.Vector3> = {
  iso: new THREE.Vector3(1, -1, 1).normalize(), // SE isometric: the drawing's opening view
  top: new THREE.Vector3(0, -1e-4, 1).normalize(),
  front: new THREE.Vector3(0, -1, 0),
  right: new THREE.Vector3(1, 0, 0),
};

/** Standing a Y-up model upright is +90° about X: (x, y, z) → (x, −z, y). */
const UPRIGHT = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), Math.PI / 2);

// sRGB bytes → linear bytes: three.js treats vertex colors as linear.
const TO_LINEAR = Uint8Array.from({ length: 256 }, (_, i) => Math.round(255 * new THREE.Color().setRGB(i / 255, 0, 0, THREE.SRGBColorSpace).r));
const linear = (c: Uint8Array) => {
  for (let i = 0; i < c.length; i++) c[i] = TO_LINEAR[c[i]];
  return c;
};

const FOV = 38;
/** Dimension label numbers: up to 3 decimals, grouped thousands (like the result card). */
const LENGTH = new Intl.NumberFormat("en-US", { maximumFractionDigits: 3 });
const MARGIN = 1.35; // room for the dimension labels around the model

export class Viewer {
  private renderer: THREE.WebGLRenderer;
  private labels: CSS2DRenderer;
  private scene = new THREE.Scene();
  private persp = new THREE.PerspectiveCamera(FOV, 1, 0.01, 1000);
  private ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.01, 1000);
  private camera: THREE.PerspectiveCamera | THREE.OrthographicCamera = this.persp;
  private controls: OrbitControls;
  /** The preview geometry; rotated (not rebuilt) when the up direction changes. */
  private content = new THREE.Group();
  private dims = new THREE.Group();
  private grid: THREE.Group | null = null;
  private layers = new Map<number, THREE.Object3D[]>();
  /** Each layer's extent in the preview's own space: the dimensions measure what is shown. */
  private layerBoxes = new Map<number, THREE.Box3>();
  private edgeObjects: THREE.LineSegments[] = [];
  private box = new THREE.Box3();
  private radius = 1;
  private frame = 0;
  private edgesOn = false;
  private hasContent = false;
  private unit = "";
  /** Display lengths = model lengths × this (the unit lengths are shown in). */
  private unitFactor = 1;
  private hidden = new Set<number>();
  private theme: ViewerTheme = { grid: "#ddd8ce", gridMajor: "#cbc4b7", dim: "#2b55c7", edge: "#1c1d1f" };
  private resizeObserver: ResizeObserver;
  /** Orientation the current buffers were built in, and the one shown. */
  private builtUp: UpAxis = "as_is";
  private shownUp: UpAxis = "as_is";
  private tween: { step: (t: number) => void; start: number; ms: number; done?: () => void } | null = null;
  private dimLabels: { obj: CSS2DObject; extent: number }[] = [];
  /** Called after each rendered frame with the camera's current axis directions. */
  onAxes: ((axes: AxisDirs) => void) | null = null;
  /** Called when the model is clicked (not dragged): the layer under the pointer, or null. */
  onPick: ((layer: number | null) => void) | null = null;
  /** The layer shown in full while every other one is dimmed; null shows all alike. */
  private focus: number | null = null;
  /** Shared stand-ins for the dimmed layers (owned here, disposed with the viewer). */
  private dimmed = {
    surface: new THREE.MeshStandardMaterial({ color: 0xd9d5cd, transparent: true, opacity: 0.22, depthWrite: false, side: THREE.DoubleSide, flatShading: true }),
    line: new THREE.LineBasicMaterial({ color: 0xb9b3a7, transparent: true, opacity: 0.3, depthWrite: false }),
    point: new THREE.PointsMaterial({ color: 0xb9b3a7, size: 4, sizeAttenuation: false, transparent: true, opacity: 0.3, depthWrite: false }),
  };
  private raycaster = new THREE.Raycaster();
  private pressed: { x: number; y: number } | null = null;
  private inv = new THREE.Quaternion();
  private key = new THREE.DirectionalLight(0xffffff, 1.7);
  private labelSize = new WeakMap<HTMLElement, { w: number; h: number }>();

  constructor(private host: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.domElement.className = "absolute inset-0 size-full outline-none";
    this.renderer.domElement.setAttribute("aria-label", "3D preview");
    host.appendChild(this.renderer.domElement);

    this.labels = new CSS2DRenderer();
    // `isolate`: CSS2DRenderer sets z-indexes on labels; keep them below the UI overlays.
    this.labels.domElement.className = "pointer-events-none absolute inset-0 isolate overflow-hidden";
    host.appendChild(this.labels.domElement);

    for (const c of [this.persp, this.ortho]) c.up.set(0, 0, 1);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.zoomToCursor = true;
    // CAD habits: middle-drag pans (shift+middle orbits), right-drag pans too.
    this.controls.mouseButtons = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.PAN, RIGHT: THREE.MOUSE.PAN };
    this.controls.addEventListener("change", () => this.requestRender());
    // Grabbing the view ends any animation at its end state.
    this.controls.addEventListener("start", () => this.finishTween());

    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x7d8594, 1.5));
    this.key.position.set(1, -1.4, 2);
    this.persp.add(this.key);
    this.scene.add(this.persp, this.content, this.dims);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(host);

    // A click picks a layer; a drag (orbit, pan) doesn't.
    const canvas = this.renderer.domElement;
    canvas.addEventListener("pointerdown", (e) => {
      this.pressed = e.button === 0 ? { x: e.clientX, y: e.clientY } : null;
    });
    canvas.addEventListener("pointerup", (e) => {
      const p = this.pressed;
      this.pressed = null;
      if (p && e.button === 0 && Math.hypot(e.clientX - p.x, e.clientY - p.y) < 5) this.onPick?.(this.pick(e.clientX, e.clientY));
    });
  }

  setTheme(theme: ViewerTheme): void {
    this.theme = theme;
    for (const e of this.edgeObjects) (e.material as THREE.LineBasicMaterial).color.set(theme.edge);
    this.rebuildGrid();
    this.rebuildDims();
    this.requestRender();
  }

  /** Show new preview buffers, built in orientation `up`. */
  show(p: PreviewBuffers, up: UpAxis, refit: boolean): void {
    this.clear();
    this.tween = null;
    this.builtUp = this.shownUp = up;
    this.content.quaternion.identity();
    const position = new THREE.BufferAttribute(p.positions, 3);
    const color = new THREE.BufferAttribute(linear(p.colors), 3, true);
    const surface = new THREE.MeshStandardMaterial({
      vertexColors: true,
      flatShading: true, // facets are the truth; never smooth them away
      side: THREE.DoubleSide,
      roughness: 0.78,
      metalness: 0,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    const edgeMat = new THREE.LineBasicMaterial({ color: this.theme.edge, transparent: true, opacity: 0.4 });
    // The layers share one vertex buffer, so their extents come from their own elements.
    const extend = (layer: number, pos: ArrayLike<number>, at: (k: number) => number, count: number) => {
      const b = this.layerBoxes.get(layer) ?? new THREE.Box3();
      const v = new THREE.Vector3();
      for (let k = 0; k < count; k++) b.expandByPoint(v.fromArray(pos as number[], at(k) * 3));
      this.layerBoxes.set(layer, b);
    };
    const add = (layer: number, ...objs: THREE.Object3D[]) => {
      for (const o of objs) o.userData.layer = layer;
      this.content.add(...objs);
      this.layers.set(layer, [...(this.layers.get(layer) ?? []), ...objs]);
    };

    for (let g = 0; g < p.groups.length; g += 5) {
      const [layer, i0, ic, e0, ec] = p.groups.subarray(g, g + 5);
      extend(layer, p.positions, (k) => p.indices[i0 + k], ic);
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", position);
      geo.setAttribute("color", color);
      geo.setIndex(new THREE.BufferAttribute(p.indices.subarray(i0, i0 + ic), 1));
      const eg = new THREE.BufferGeometry();
      eg.setAttribute("position", position);
      eg.setIndex(new THREE.BufferAttribute(p.edges.subarray(e0, e0 + ec), 1));
      const edges = new THREE.LineSegments(eg, edgeMat);
      edges.userData.edges = true;
      this.edgeObjects.push(edges);
      add(layer, new THREE.Mesh(geo, surface), edges);
    }
    if (p.lines.length) {
      const pos = new THREE.BufferAttribute(p.lines, 3);
      const col = new THREE.BufferAttribute(linear(p.lineColors), 3, true);
      const mat = new THREE.LineBasicMaterial({ vertexColors: true });
      for (let g = 0; g < p.lineGroups.length; g += 3) {
        const [layer, start, count] = p.lineGroups.subarray(g, g + 3);
        extend(layer, p.lines, (k) => start + k, count);
        const geo = new THREE.BufferGeometry();
        geo.setAttribute("position", pos);
        geo.setAttribute("color", col);
        geo.setDrawRange(start, count);
        add(layer, new THREE.LineSegments(geo, mat));
      }
    }
    if (p.points.length) {
      const pos = new THREE.BufferAttribute(p.points, 3);
      const col = new THREE.BufferAttribute(linear(p.pointColors), 3, true);
      const mat = new THREE.PointsMaterial({ vertexColors: true, size: 5, sizeAttenuation: false });
      for (let g = 0; g < p.pointGroups.length; g += 3) {
        const [layer, start, count] = p.pointGroups.subarray(g, g + 3);
        extend(layer, p.points, (k) => start + k, count);
        const geo = new THREE.BufferGeometry();
        geo.setAttribute("position", pos);
        geo.setAttribute("color", col);
        geo.setDrawRange(start, count);
        add(layer, new THREE.Points(geo, mat));
      }
    }

    this.applyVisibility();
    this.applyFocus();
    this.measure();
    const size = this.box.getSize(new THREE.Vector3());
    this.radius = Math.max(size.length() / 2, 1e-6);
    for (const c of [this.persp, this.ortho]) {
      c.near = this.radius / 1000;
      c.far = this.radius * 1000;
      c.updateProjectionMatrix();
    }
    this.rebuildGrid();
    this.rebuildDims();
    if (refit) this.setView("iso", false);
    else this.requestRender();
  }

  /** Show the model in orientation `up`, turning the existing preview (no rebuild). */
  setOrientation(up: UpAxis, animate = true): void {
    this.finishTween();
    if (up === this.shownUp) return;
    this.shownUp = up;
    const target = up === this.builtUp ? new THREE.Quaternion() : this.builtUp === "as_is" ? UPRIGHT.clone() : UPRIGHT.clone().invert();
    const from = this.content.quaternion.clone();
    const finish = () => {
      this.content.quaternion.copy(target);
      this.measure();
      this.rebuildGrid();
      this.rebuildDims();
    };
    // Hide the dimensions while turning; they are rebuilt for the new box.
    for (const c of this.dims.children) c.visible = false;
    this.animate(
      animate ? 380 : 0,
      (t) => {
        this.content.quaternion.slerpQuaternions(from, target, t);
      },
      finish,
    );
  }

  /** Unit suffix for dimension labels ("mm", "in", or "" for unitless), and the factor
   *  from model lengths to that unit (display only). */
  setUnit(unit: string, factor = 1): void {
    this.unit = unit;
    this.unitFactor = factor;
    this.rebuildDims();
    this.requestRender();
  }

  setEdges(on: boolean): void {
    this.edgesOn = on;
    this.applyVisibility();
  }

  /** Hide exactly these layers (and show every other one). */
  setHidden(layers: Iterable<number>): void {
    this.hidden = new Set(layers);
    this.applyVisibility();
  }

  /** Show `layer` in full and dim the others; null shows every layer alike. */
  highlight(layer: number | null): void {
    if (layer === this.focus) return;
    this.focus = layer;
    this.applyFocus();
  }

  /** Perspective or orthographic projection, keeping what is on screen. */
  setOrtho(on: boolean): void {
    const next = on ? this.ortho : this.persp;
    if (next === this.camera) return;
    const prev = this.camera;
    const dist = prev.position.distanceTo(this.controls.target);
    next.position.copy(prev.position);
    next.quaternion.copy(prev.quaternion);
    if (next === this.ortho) {
      // Match the perspective view's height at the target.
      const h = 2 * dist * Math.tan(THREE.MathUtils.degToRad(FOV / 2));
      this.ortho.zoom = 1;
      this.orthoFrustum(h / 2);
    } else {
      // Move the camera so the target keeps its on-screen size.
      const h = (this.ortho.top - this.ortho.bottom) / this.ortho.zoom;
      const d = h / 2 / Math.tan(THREE.MathUtils.degToRad(FOV / 2));
      const dir = next.position.clone().sub(this.controls.target).normalize();
      next.position.copy(this.controls.target).addScaledVector(dir, d);
    }
    this.scene.remove(prev);
    this.scene.add(next);
    next.add(this.key);
    this.camera = next;
    this.controls.object = next;
    this.resize();
    this.controls.update();
    this.requestRender();
  }

  get isOrtho(): boolean {
    return this.camera === this.ortho;
  }

  /** Look at the whole model from a named direction. */
  setView(view: ViewName, animate = true): void {
    this.finishTween();
    const dir = VIEWS[view];
    const dist = (this.radius / Math.sin(THREE.MathUtils.degToRad(FOV / 2))) * MARGIN;
    const fromPos = this.camera.position.clone();
    const fromTarget = this.controls.target.clone();
    const toPos = dir.clone().multiplyScalar(dist);
    const fromDir = fromPos.clone().sub(fromTarget);
    const fromLen = fromDir.length();
    fromDir.normalize();
    const fromZoom = this.orthoHome();
    this.animate(animate ? 420 : 0, (t) => {
      // Swing the direction (keeps the model in view) while moving the target home.
      const d = new THREE.Vector3().copy(fromDir).lerp(dir, t).normalize();
      const len = THREE.MathUtils.lerp(fromLen, dist, t);
      this.controls.target.lerpVectors(fromTarget, new THREE.Vector3(), t);
      this.camera.position.copy(this.controls.target).addScaledVector(d, len);
      if (this.camera === this.ortho) {
        this.ortho.zoom = THREE.MathUtils.lerp(fromZoom, 1, t);
        this.ortho.updateProjectionMatrix();
      }
      this.camera.lookAt(this.controls.target);
    }, () => {
      this.camera.position.copy(toPos);
      this.controls.target.set(0, 0, 0);
      this.controls.update();
    });
  }

  /** Frame the whole model, keeping the current viewing direction. */
  fit(): void {
    this.finishTween();
    const dir = this.camera.position.clone().sub(this.controls.target).normalize();
    const dist = (this.radius / Math.sin(THREE.MathUtils.degToRad(FOV / 2))) * MARGIN;
    const from = this.camera.position.clone();
    const fromTarget = this.controls.target.clone();
    const fromZoom = this.orthoHome();
    this.animate(320, (t) => {
      this.controls.target.lerpVectors(fromTarget, new THREE.Vector3(), t);
      this.camera.position.lerpVectors(from, dir.clone().multiplyScalar(dist), t);
      if (this.camera === this.ortho) {
        this.ortho.zoom = THREE.MathUtils.lerp(fromZoom, 1, t);
        this.ortho.updateProjectionMatrix();
      }
      this.camera.lookAt(this.controls.target);
    }, () => this.controls.update());
  }

  /** Reset the orthographic frustum to frame the model at zoom 1, and return the zoom
   *  that shows what is on screen now (to animate from). */
  private orthoHome(): number {
    const visible = (this.ortho.top - this.ortho.bottom) / 2 / this.ortho.zoom;
    this.orthoFrustum(this.radius * MARGIN);
    this.ortho.zoom = visible > 0 ? (this.radius * MARGIN) / visible : 1;
    this.ortho.updateProjectionMatrix();
    return this.ortho.zoom;
  }

  dispose(): void {
    cancelAnimationFrame(this.frame);
    this.resizeObserver.disconnect();
    this.clear();
    for (const m of Object.values(this.dimmed)) m.dispose();
    this.controls.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
    this.labels.domElement.remove();
  }

  // ------------------------------------------------------------------ internals

  /** Run `step(t)` for t in 0..1 over `ms` (eased), then `done`. Callers capture their
   *  start state after `finishTween()`, so animations never jump back. */
  private animate(ms: number, step: (t: number) => void, done?: () => void): void {
    this.finishTween();
    if (ms <= 0 || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      step(1);
      done?.();
      this.requestRender();
      return;
    }
    this.tween = { step, start: performance.now(), ms, done };
    this.requestRender();
  }

  private finishTween(): void {
    const t = this.tween;
    if (!t) return;
    this.tween = null;
    t.step(1);
    t.done?.();
    this.requestRender();
  }

  private orthoFrustum(halfHeight: number): void {
    const { clientWidth: w, clientHeight: h } = this.host;
    const aspect = w && h ? w / h : 1;
    Object.assign(this.ortho, { left: -halfHeight * aspect, right: halfHeight * aspect, top: halfHeight, bottom: -halfHeight });
    this.ortho.updateProjectionMatrix();
  }

  /** Bounds of what is shown, in world space (exact for the 90° turns we apply). */
  private measure(): void {
    this.content.updateMatrixWorld(true);
    this.box.setFromObject(this.content);
    this.hasContent = !this.box.isEmpty();
    if (!this.hasContent) this.box.set(new THREE.Vector3(-1, -1, -1), new THREE.Vector3(1, 1, 1));
  }

  /** The layer under a screen point: the nearest visible face, line or point. */
  private pick(clientX: number, clientY: number): number | null {
    const r = this.renderer.domElement.getBoundingClientRect();
    const ndc = new THREE.Vector2(((clientX - r.left) / r.width) * 2 - 1, -((clientY - r.top) / r.height) * 2 + 1);
    this.raycaster.setFromCamera(ndc, this.camera);
    this.raycaster.params.Line.threshold = this.radius / 150;
    this.raycaster.params.Points.threshold = this.radius / 100;
    const targets: THREE.Object3D[] = [];
    for (const objs of this.layers.values()) for (const o of objs) if (o.visible && !o.userData.edges) targets.push(o);
    const hit = this.raycaster.intersectObjects(targets, false)[0];
    return hit ? (hit.object.userData.layer as number) : null;
  }

  /** Dim every layer but the focused one (by swapping in the shared dimmed materials). */
  private applyFocus(): void {
    for (const [layer, objs] of this.layers) {
      for (const o of objs) {
        const shape = o as THREE.Mesh | THREE.LineSegments | THREE.Points;
        shape.userData.base ??= shape.material;
        const dim = this.focus !== null && layer !== this.focus;
        if (!dim) shape.material = shape.userData.base;
        else if (shape instanceof THREE.Mesh) shape.material = this.dimmed.surface;
        else if (shape instanceof THREE.Points) shape.material = this.dimmed.point;
        else shape.material = this.dimmed.line;
      }
    }
    this.requestRender();
  }

  private applyVisibility(): void {
    for (const [layer, objs] of this.layers) {
      const shown = !this.hidden.has(layer);
      for (const o of objs) o.visible = o.userData.edges ? shown && this.edgesOn : shown;
    }
    this.rebuildDims();
    this.requestRender();
  }

  /** The extent of the layers shown (the whole model when none is hidden). */
  private shownBox(): THREE.Box3 {
    if (!this.hidden.size) return this.box;
    this.content.updateMatrixWorld(true);
    const b = new THREE.Box3();
    for (const [layer, lb] of this.layerBoxes) if (!this.hidden.has(layer)) b.union(lb);
    return b.isEmpty() ? b : b.applyMatrix4(this.content.matrixWorld);
  }

  private rebuildGrid(): void {
    if (this.grid) {
      this.scene.remove(this.grid);
      this.grid.traverse((o) => {
        if (o instanceof THREE.LineSegments) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        }
      });
    }
    // Drafting grid on CAD's XY plane under the model: minor + major lines.
    const step = 10 ** Math.floor(Math.log10(this.radius / 2));
    const extent = Math.ceil((this.radius * 4) / (step * 5)) * step * 5;
    const group = new THREE.Group();
    const make = (spacing: number, color: string, opacity: number) => {
      const pts: number[] = [];
      for (let v = -extent; v <= extent + 1e-9; v += spacing) pts.push(v, -extent, 0, v, extent, 0, -extent, v, 0, extent, v, 0);
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
      return new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color, transparent: true, opacity }));
    };
    group.add(make(step, this.theme.grid, 0.9), make(step * 5, this.theme.gridMajor, 1));
    group.position.z = this.hasContent ? this.box.min.z : 0;
    this.grid = group;
    this.scene.add(group);
  }

  /** Bounding-box dimension lines with mono labels, drawn off the model's near edges. */
  private rebuildDims(): void {
    for (const c of [...this.dims.children]) {
      this.dims.remove(c);
      if (c instanceof CSS2DObject) c.element.remove();
      if (c instanceof THREE.LineSegments) {
        c.geometry.dispose();
        (c.material as THREE.Material).dispose();
      }
    }
    this.dimLabels = [];
    const shown = this.shownBox();
    if (!this.hasContent || shown.isEmpty()) return;
    const { min, max } = shown;
    const off = this.radius * 0.08;
    const tick = this.radius * 0.025;
    const flat = this.radius * 1e-9;
    const pts: number[] = [];
    const seg = (a: THREE.Vector3, b: THREE.Vector3) => pts.push(a.x, a.y, a.z, b.x, b.y, b.z);
    const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);
    const label = (at: THREE.Vector3, value: number) => {
      const el = document.createElement("div");
      el.className = "dim-label";
      const shown = LENGTH.format(value * this.unitFactor);
      el.textContent = this.unit ? `${shown} ${this.unit}` : shown;
      const obj = new CSS2DObject(el);
      obj.position.copy(at);
      this.dims.add(obj);
      this.dimLabels.push({ obj, extent: value });
    };
    const dim = (a: THREE.Vector3, b: THREE.Vector3, tickDir: THREE.Vector3) => {
      const extent = a.distanceTo(b);
      if (extent <= flat) return; // no dimension across a flat side
      seg(a, b);
      const t = tickDir.clone().multiplyScalar(tick);
      seg(a.clone().sub(t), a.clone().add(t));
      seg(b.clone().sub(t), b.clone().add(t));
      label(a.clone().add(b).multiplyScalar(0.5), extent);
    };
    const y0 = min.y - off;
    dim(V(min.x, y0, min.z), V(max.x, y0, min.z), V(0, 1, 0)); // X along the front edge
    const x0 = max.x + off;
    dim(V(x0, min.y, min.z), V(x0, max.y, min.z), V(1, 0, 0)); // Y along the right edge
    const xz = min.x - off;
    dim(V(xz, min.y, min.z), V(xz, min.y, max.z), V(1, 0, 0)); // Z up the front-left corner

    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    this.dims.add(new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: this.theme.dim })));
  }

  /** Nudge dimension labels apart when they would overlap on screen. */
  private separateLabels(): void {
    const { clientWidth: w, clientHeight: h } = this.host;
    const placed: { x: number; y: number; w: number; h: number }[] = [];
    for (const { obj } of this.dimLabels) {
      obj.center.set(0.5, 0.5);
      const p = obj.getWorldPosition(new THREE.Vector3()).project(this.camera);
      const el = obj.element;
      let size = this.labelSize.get(el);
      if (!size && el.isConnected && el.offsetWidth) this.labelSize.set(el, (size = { w: el.offsetWidth + 6, h: el.offsetHeight + 4 }));
      if (!size) continue;
      const box = { x: ((p.x + 1) / 2) * w, y: ((1 - p.y) / 2) * h, ...size };
      let shift = 0;
      while (shift < 4 && placed.some((b) => Math.abs(b.x - box.x) < (b.w + box.w) / 2 && Math.abs(b.y - box.y - shift * box.h) < (b.h + box.h) / 2)) shift++;
      if (shift) obj.center.set(0.5, 0.5 - shift);
      placed.push({ ...box, y: box.y + shift * box.h });
    }
  }

  private clear(): void {
    const materials = new Set<THREE.Material>();
    const geometries = new Set<THREE.BufferGeometry>();
    for (const c of [...this.content.children]) {
      this.content.remove(c);
      if (c instanceof THREE.Mesh || c instanceof THREE.LineSegments || c instanceof THREE.Points) {
        geometries.add(c.geometry);
        materials.add((c.userData.base ?? c.material) as THREE.Material); // never the shared dimmed ones
      }
    }
    for (const g of geometries) g.dispose();
    for (const m of materials) m.dispose();
    this.layers.clear();
    this.layerBoxes.clear();
    this.hidden.clear();
    this.edgeObjects = [];
    this.box.makeEmpty();
    this.hasContent = false;
    this.rebuildDims();
  }

  private resize(): void {
    const { clientWidth: w, clientHeight: h } = this.host;
    if (!w || !h) return;
    this.renderer.setSize(w, h, false);
    this.labels.setSize(w, h);
    this.persp.aspect = w / h;
    this.persp.updateProjectionMatrix();
    this.orthoFrustum((this.ortho.top - this.ortho.bottom) / 2 || this.radius * MARGIN);
    this.requestRender();
  }

  private requestRender(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      if (this.tween) {
        const t = Math.min(1, (performance.now() - this.tween.start) / this.tween.ms);
        const eased = 1 - (1 - t) ** 3;
        this.tween.step(eased);
        if (t >= 1) {
          const done = this.tween.done;
          this.tween = null;
          done?.();
        }
        this.requestRender();
      } else if (this.controls.update()) this.requestRender(); // keep damping until settled
      this.renderer.render(this.scene, this.camera);
      this.separateLabels();
      this.labels.render(this.scene, this.camera);
      if (this.onAxes) {
        this.inv.copy(this.camera.quaternion).invert();
        const dir = (x: number, y: number, z: number) => {
          const v = new THREE.Vector3(x, y, z).applyQuaternion(this.inv);
          return [v.x, v.y, v.z] as [number, number, number];
        };
        this.onAxes({ x: dir(1, 0, 0), y: dir(0, 1, 0), z: dir(0, 0, 1) });
      }
    });
  }
}
