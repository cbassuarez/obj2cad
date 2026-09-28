# obj2cad — Plan

In-house OBJ → DWG/DXF converter for Ideum devs and designers. Replaces outsourced conversion.
Priorities, in order: **trustworthy → no surprises → easy → beautiful → fast.**

Repo: `github.com/cbassuarez/obj2cad` (public). Status: v0.1 live at https://cbassuarez.com/obj2cad/ (2026-09-28). UI: single light "drafting" theme.

---

## 1. Decisions

| Topic | Decision |
|---|---|
| Delivery | Web app, conversion runs **in the browser** (Rust → WebAssembly in a Web Worker). Files never leave the machine. Hosted on GitHub Pages, installable/offline PWA. |
| Outputs | DWG and DXF. Default version **2018 (AC1032)**; 2013/2010 selectable. More formats later as a curated set. |
| Parity | Vertices, faces, n-gons, and structure preserved **bit-for-bit** by default. |
| Curves | **Strict** by default (see §3); user may loosen to a stated tolerance, and loosened output is flagged in the file, UI, and report. Regions that fail the rule stay exactly faceted. The tool never approximates silently. |
| Structure | `o`/`g` → layers; `usemtl` → entity true color from MTL `Kd`. |
| Units / axis | Chosen automatically, never asked: up direction detected from geometry (resting base, thin axis; exporter only as fallback), units from exporter convention or size (unitless if no basis). House unit for files that don't state one; resulting size shown next to the download; one-click override. Units written to `$INSUNITS`. |
| Acceptance | Automated independent-reader checks every commit + teammate AutoCAD checklist every release. |

## 2. Architecture

```
crates/
  obj2cad-core     strict lossless OBJ parser, mesh (half-edge), diagnostics
  obj2cad-curves   segmentation, primitive + B-spline fitting, verification
  obj2cad-acis     ACIS B-rep IR → SAT (text) / SAB (binary)
  obj2cad-dxf      own DXF writer (full control of number formatting)
  obj2cad-dwg      DWG via acadrust (MPL-2.0), pinned + audited
  obj2cad-wasm     wasm-bindgen API for the web app
web/               Vite + TypeScript UI, three.js viewer, Web Worker, PWA
tests/             corpus fetch scripts, edge-case fixtures, verification harness
```

### Exact path (default)
- Parser keeps each coordinate's **original decimal token** alongside its f64. DXF writes the original token when it round-trips to the same f64 (else shortest round-trip via `ryu`); DWG stores the raw f64 bits. Either way the reader gets bit-identical doubles.
- Never welds, re-orders, triangulates, or drops anything silently. Every non-carried item (normals, UVs, textures, `l`/`p` elements, smoothing groups) is listed in the report.
- Entity: `MESH` (keeps shared vertices and n-gons, ≈16.7M faces). Optional faceted `3DSOLID` for watertight, manifold meshes. `3DFACE` fallback for very old consumers. No polyface meshes (32,767-vertex cap).

### Curve path
1. Segment the mesh into candidate regions (normal/curvature region growing, feature-edge breaks).
2. Fit plane / cylinder / cone / sphere / torus (RANSAC seed → Levenberg–Marquardt), then B-spline surfaces for freeform regions.
3. **Verify every original vertex** against the fitted surface under the active rule. Pass → curved ACIS face. Fail → region stays faceted.
4. Boundaries between two recognized surfaces use their exact intersection curve; boundaries touching facets use the original edge chain lifted onto the surface.
5. Emit as ACIS inside `3DSOLID`/`SURFACE`/`BODY` (SAB for 2013+, SAT for 2010).

## 3. Parity rules

- **Exact:** output doubles == input doubles, checked per coordinate.
- **Strict curves:** each vertex lies on the surface within the *precision the OBJ itself states*: a coordinate written as `1.234567` is only known to ±0.5e-6, so a surface inside that band is indistinguishable from the source. Floor: a few ULPs for full-precision files. This makes strict mode meaningful for real exporters (most write ~6 decimals) without ever inventing precision.
- **Tolerance curves:** user-set absolute tolerance in model units. Written into the file's custom drawing properties and the report.
- Every export writes a **parity report** (JSON + in-app view): counts, bbox, geometry hash of input and read-back output, per-region surface type / max deviation / rule used, everything dropped. The report hash and engine version go into the drawing's custom properties so any file can be traced back and reproduced.

## 4. Verification (trust)

- **Independent read-back** in CI: every corpus output is re-read by ezdxf (Python, independent implementation) and LibreDWG, compared bit-for-bit against the source OBJ. Curved faces are re-sampled and checked against the original vertices.
- ODA File Converter audit pass in CI (used as a test tool only, never shipped; license to be confirmed).
- Property-based fuzzing of the OBJ parser; golden-file snapshots of DXF output.
- **Curve benchmark on ABC:** ABC ships ground-truth surface types and parameters per patch. Track recall (true patches recovered), parameter error, and **false positives, which must be zero** (guaranteed by step 3, measured anyway).
- Release gate: teammate opens the release checklist files in AutoCAD (open, AUDIT, LIST, MASSPROP on solids, visual compare) and signs off in the release PR.

## 5. Test corpus

| Set | Source | License | Stored |
|---|---|---|---|
| CAD with ground truth | ABC dataset chunk 0000 (obj + feat + step), curated to ~200 models | MIT | Curated subset as a GitHub release asset; raw chunk (~14 GB) never committed |
| Edge cases | Hand-built: negative/relative indices, n-gons, non-planar, degenerate, duplicate verts, huge coords, CRLF, missing MTL, unicode, `l`/`p`, freeform statements | ours | Committed |
| Scans & sculpts | Stanford scans, common-3d-test-models, Blender/Khronos samples | mixed; Stanford is non-commercial | Fetched on demand for local runs only |

## 6. Release and update

- Conventional commits → release-please opens a release PR (changelog + version bump) → merge tags the release.
- Tag pipeline: build wasm + web, run full verification, deploy to GitHub Pages. Each release is also kept at `/v/<version>/` so old exports are reproducible.
- **Auto-update:** a service worker detects the new version and shows "Update available — reload". It never reloads mid-conversion.
- Release notes, checksums, and an SBOM attached to every GitHub Release; GitHub artifact attestations for the build.
- No code-signing needed (web). A CLI can be added later from the same core.

## 7. Phases

| Phase | Deliverable |
|---|---|
| 0 | Repo, CI, corpus scripts, edge-case fixtures, verification harness |
| 1 | Strict OBJ parser + exact `MESH` DXF + parity report; web MVP (drop → preview → export) |
| 2 | DWG via acadrust, faceted `3DSOLID`, AutoCAD acceptance round 1 → **v0.1 public** |
| 3 | Analytic surfaces (plane/cyl/cone/sphere/torus), ACIS writer, strict + tolerance modes, deviation heatmap, ABC benchmark |
| 4 | Freeform B-spline regions, exact trims, watertight curved solids |
| 5 | Performance for very large meshes, more formats, optional CLI |

## 8. Findings during build

- **acadrust's DXF writer is not exact.** It formats reals with `{:.16}` (16 fixed decimals), so
  values in (1e-15, 1) lose significant digits (e.g. `1.2345678901234567e-5` → 12 digits), and
  NaN/Inf become `0.0` silently. We therefore write DXF ourselves and use acadrust only for DWG,
  where doubles are stored as raw bits (still verified by read-back).
- **Real-world OBJs carry ~6 decimals** (confirmed on ABC and Blender files), which is why strict
  curve mode is defined relative to the file's stated precision (§3).
- **MESH needs a CLASS entry** in DXF that ezdxf omits; we add the one AutoCAD writes. Verify
  in the first AutoCAD acceptance round.

## 9. Known risks

- **Stitching curved and faceted regions** into one valid watertight solid is the hardest problem. Fallback: separate sheet bodies per region with a reported gap until phase 4 closes it.
- **acadrust maturity** (v0.5.x). Mitigation: pinned version, independent read-back, AutoCAD sign-off; ODA SDK as a paid escape hatch if DWG output ever fails acceptance.
- **Browser memory** (wasm32 ≈ 4 GB) caps mesh size around ~10M triangles; the UI states the limit up front.
- **OBJ freeform (NURBS) input** is supported by the parser but has no real-world fixtures yet.
