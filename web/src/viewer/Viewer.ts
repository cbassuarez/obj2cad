// 3D preview in CAD space (Z up). Display only: float32 buffers re-centered on the model's
// bounds; the exported file is produced separately from the exact double-precision data.
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { CSS2DObject, CSS2DRenderer } from "three/addons/renderers/CSS2DRenderer.js";

export interface Preview {
  positions: Float32Array;
  indices: Uint32Array;
  edges: Uint32Array;
  colors: Uint8Array;
  lines: Float32Array;
  points: Float32Array;
  /** Per mesh: [layer, indexStart, indexCount, edgeStart, edgeCount]. */
  groups: Uint32Array;
}

export interface ViewerTheme {
  grid: string;
  gridMajor: string;
  dim: string;
  edge: string;
  line: string;
}

/** View-space direction of each world axis (x right, y up), for the axis gizmo. */
export type AxisDirs = { x: [number, number, number]; y: [number, number, number]; z: [number, number, number] };

export interface Dimensions {
  size: [number, number, number];
}

export class Viewer {
  private renderer: THREE.WebGLRenderer;
  private labels: CSS2DRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(38, 1, 0.01, 1000);
  private controls: OrbitControls;
  private content = new THREE.Group();
  private dims = new THREE.Group();
  private grid: THREE.Group | null = null;
  private layers = new Map<number, THREE.Object3D[]>();
  private edgeObjects: THREE.LineSegments[] = [];
  private box = new THREE.Box3();
  private radius = 1;
  private frame = 0;
  private edgesOn = false;
  /** False when there is nothing to draw: no dimensions for a placeholder box. */
  private hasContent = false;
  private unit = "";
  private hidden = new Set<number>();
  private theme: ViewerTheme = { grid: "#1c1f24", gridMajor: "#272b32", dim: "#8fb0ff", edge: "#c6f432", line: "#8fb0ff" };
  private resizeObserver: ResizeObserver;
  /** Called after each rendered frame with the camera's current axis directions. */
  onAxes: ((axes: AxisDirs) => void) | null = null;
  private inv = new THREE.Quaternion();

  constructor(private host: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.domElement.className = "absolute inset-0 size-full outline-none";
    this.renderer.domElement.setAttribute("aria-label", "3D preview. Drag to orbit, scroll to zoom.");
    host.appendChild(this.renderer.domElement);

    this.labels = new CSS2DRenderer();
    // `isolate`: CSS2DRenderer sets z-indexes on labels; keep them below the UI overlays.
    this.labels.domElement.className = "pointer-events-none absolute inset-0 isolate overflow-hidden";
    host.appendChild(this.labels.domElement);

    this.camera.up.set(0, 0, 1);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.addEventListener("change", () => this.requestRender());

    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x7d8594, 1.5));
    const key = new THREE.DirectionalLight(0xffffff, 1.7);
    key.position.set(1, -1.4, 2);
    this.camera.add(key);
    this.scene.add(this.camera, this.content, this.dims);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(host);
  }

  setTheme(theme: ViewerTheme): void {
    this.theme = theme;
    for (const e of this.edgeObjects) (e.material as THREE.LineBasicMaterial).color.set(theme.edge);
    this.rebuildGrid();
    this.rebuildDims();
    this.requestRender();
  }

  show(p: Preview): Dimensions {
    this.clear();
    const position = new THREE.BufferAttribute(p.positions, 3);
    const color = new THREE.BufferAttribute(p.colors, 3, true);
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

    for (let g = 0; g < p.groups.length; g += 5) {
      const [layer, i0, ic, e0, ec] = p.groups.subarray(g, g + 5);
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", position);
      geo.setAttribute("color", color);
      geo.setIndex(new THREE.BufferAttribute(p.indices.subarray(i0, i0 + ic), 1));
      const mesh = new THREE.Mesh(geo, surface);
      const eg = new THREE.BufferGeometry();
      eg.setAttribute("position", position);
      eg.setIndex(new THREE.BufferAttribute(p.edges.subarray(e0, e0 + ec), 1));
      const edges = new THREE.LineSegments(eg, edgeMat);
      this.edgeObjects.push(edges);
      this.content.add(mesh, edges);
      const list = this.layers.get(layer) ?? [];
      list.push(mesh, edges);
      this.layers.set(layer, list);
    }
    if (p.points.length) {
      const pg = new THREE.BufferGeometry();
      pg.setAttribute("position", new THREE.BufferAttribute(p.points, 3));
      this.content.add(new THREE.Points(pg, new THREE.PointsMaterial({ color: this.theme.line, size: 6, sizeAttenuation: false })));
    }
    if (p.lines.length) {
      const lg = new THREE.BufferGeometry();
      lg.setAttribute("position", new THREE.BufferAttribute(p.lines, 3));
      this.content.add(new THREE.LineSegments(lg, new THREE.LineBasicMaterial({ color: this.theme.line })));
    }

    this.applyVisibility();
    this.box.setFromObject(this.content, true);
    this.hasContent = !this.box.isEmpty();
    if (!this.hasContent) this.box.set(new THREE.Vector3(-1, -1, -1), new THREE.Vector3(1, 1, 1));
    const size = this.box.getSize(new THREE.Vector3());
    this.radius = Math.max(size.length() / 2, 1e-6);
    this.camera.near = this.radius / 1000;
    this.camera.far = this.radius * 1000;
    this.camera.updateProjectionMatrix();
    this.rebuildGrid();
    this.rebuildDims();
    this.fit();
    return { size: [size.x, size.y, size.z] };
  }

  /** Unit suffix for dimension labels ("mm", "in", or "" for unitless). */
  setUnit(unit: string): void {
    this.unit = unit;
    this.rebuildDims();
    this.requestRender();
  }

  setEdges(on: boolean): void {
    this.edgesOn = on;
    this.applyVisibility();
  }

  /** Preview only: the exported file always contains every layer. */
  setLayerVisible(layer: number, visible: boolean): void {
    if (visible) this.hidden.delete(layer);
    else this.hidden.add(layer);
    this.applyVisibility();
  }

  private applyVisibility(): void {
    for (const [layer, objs] of this.layers) {
      const shown = !this.hidden.has(layer);
      for (const o of objs) o.visible = o instanceof THREE.LineSegments ? shown && this.edgesOn : shown;
    }
    this.requestRender();
  }

  fit(): void {
    const dist = (this.radius / Math.sin(THREE.MathUtils.degToRad(this.camera.fov / 2))) * 1.15;
    const dir = new THREE.Vector3(1, -1.35, 0.85).normalize();
    this.controls.target.set(0, 0, 0);
    this.camera.position.copy(dir.multiplyScalar(dist));
    this.controls.update();
    this.requestRender();
  }

  dispose(): void {
    cancelAnimationFrame(this.frame);
    this.resizeObserver.disconnect();
    this.clear();
    this.controls.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
    this.labels.domElement.remove();
  }

  // ------------------------------------------------------------------ internals

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
      for (let v = -extent; v <= extent + 1e-9; v += spacing) {
        pts.push(v, -extent, 0, v, extent, 0, -extent, v, 0, extent, v, 0);
      }
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
      return new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color, transparent: true, opacity }));
    };
    group.add(make(step, this.theme.grid, 0.9), make(step * 5, this.theme.gridMajor, 1));
    group.position.z = this.box.isEmpty() ? 0 : this.box.min.z;
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
    if (this.box.isEmpty() || !this.hasContent) return;
    const { min, max } = this.box;
    const off = this.radius * 0.08;
    const tick = this.radius * 0.025;
    const pts: number[] = [];
    const seg = (a: THREE.Vector3, b: THREE.Vector3) => pts.push(a.x, a.y, a.z, b.x, b.y, b.z);
    const label = (at: THREE.Vector3, value: number) => {
      const el = document.createElement("div");
      el.className = "dim-label";
      el.textContent = this.unit ? `${value.toFixed(3)} ${this.unit}` : value.toFixed(3);
      const obj = new CSS2DObject(el);
      obj.position.copy(at);
      this.dims.add(obj);
    };
    const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);
    // X along the front edge
    const y0 = min.y - off;
    seg(V(min.x, y0, min.z), V(max.x, y0, min.z));
    seg(V(min.x, y0 - tick, min.z), V(min.x, y0 + tick, min.z));
    seg(V(max.x, y0 - tick, min.z), V(max.x, y0 + tick, min.z));
    label(V((min.x + max.x) / 2, y0, min.z), max.x - min.x);
    // Y along the right edge
    const x0 = max.x + off;
    seg(V(x0, min.y, min.z), V(x0, max.y, min.z));
    seg(V(x0 - tick, min.y, min.z), V(x0 + tick, min.y, min.z));
    seg(V(x0 - tick, max.y, min.z), V(x0 + tick, max.y, min.z));
    label(V(x0, (min.y + max.y) / 2, min.z), max.y - min.y);
    // Z up the front-left corner
    const xz = min.x - off;
    seg(V(xz, min.y, min.z), V(xz, min.y, max.z));
    seg(V(xz - tick, min.y, min.z), V(xz + tick, min.y, min.z));
    seg(V(xz - tick, min.y, max.z), V(xz + tick, min.y, max.z));
    label(V(xz, min.y, (min.z + max.z) / 2), max.z - min.z);

    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    this.dims.add(new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: this.theme.dim })));
  }

  private clear(): void {
    for (const c of [...this.content.children]) {
      this.content.remove(c);
      if (c instanceof THREE.Mesh || c instanceof THREE.LineSegments || c instanceof THREE.Points) {
        c.geometry.dispose();
        (c.material as THREE.Material).dispose();
      }
    }
    this.layers.clear();
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
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.requestRender();
  }

  private requestRender(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      if (this.controls.update()) this.requestRender(); // keep damping until settled
      this.renderer.render(this.scene, this.camera);
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
