# obj2cad

Convert Wavefront OBJ to DWG/DXF **without changing a single bit of geometry**.

**Use it: https://cbassuarez.com/obj2cad/** (runs entirely in your browser; files are
never uploaded; installable and works offline).

> Status: pre-release (see [the plan](docs/PLAN.md)). DXF (ASCII and binary) is verified;
> DWG and curved surfaces are in beta until they pass AutoCAD acceptance.

## What "exact" means here

- Every coordinate in the output is the **same IEEE-754 double** as in the OBJ. Where
  possible, the OBJ's original number text is copied verbatim; otherwise the shortest
  form that round-trips exactly is written.
- Faces keep their vertex order and n-gons are never triangulated. Vertices are never
  welded, reordered within a face, or moved.
- Anything the target format cannot carry (UVs, normals, per-vertex colors, …) is listed
  in the report. Nothing is dropped silently.
- Ambiguous input (bad numbers, out-of-range indices, NaN/Inf) is **rejected with a line
  number**, never guessed.
- Same input + same version → byte-identical output.

Every conversion writes a `*.report.json` with input/output SHA-256, a canonical
**parity hash** of the geometry, the layer map, and all notes. The parity hash is also
stored in the drawing's custom properties (`DWGPROPS` in AutoCAD), so any file can be
traced back to its source.

## What goes in

A model (`.obj`), or a bundle: a `.zip`, a folder, or several files with the model's
material libraries (`.mtl`), texture images (`.jpg`, `.png`) and point clouds (`.xyz`).
Files are matched by name, ignoring folders and case, and a bundle becomes one drawing
(scan bundles share coordinates). Each file keeps its own layers: a name two files both
use gets the file's name added (`Chair (west)`), and the app lists layers under their file.
The report lists every file as used, not used or missing.

- **Point clouds** (`.xyz`): each point becomes a CAD point with its exact coordinate
  text, and RGB columns become its color. Column layouts that could mean two things
  (colors or normals) are rejected, not guessed. In the app, a large cloud (16 MB or
  more) loads after the models it comes with, which are shown first; the file is written
  once, from everything. Clouds over 4 million points are shown evenly thinned; the file
  has every point.
- **Colors**: a material's `Kd` becomes the entity's true color. A CAD mesh entity has one
  color, so faces with more detail than that get one color each, the color they show from
  far enough away to look uniform:
  - **Textures**: the image averaged over the face's area, about four samples per texel,
    mixed in linear light (so fine black and white stripes read as the mid gray they look
    like, not dark gray), with transparent parts left out and `-s`, `-o` and `-clamp`
    applied. Decoded by the same code in the browser and the command-line tool.
  - **Vertex colors** (`v x y z r g b`, common in scans): the average of the face's or
    line's corners, also in linear light. Points keep their own color.

  Faces are grouped into one mesh entity per color. Colors are only merged when they
  are within 4 levels of each other or a texture has more than 256 colors, and then the
  colors covering the most area are kept closest. The shapes stay exact; these colors
  are labeled approximate, and the parity harness checks each against its own reference.
- **Free-form curves** in the OBJ (`curv`, B-spline or Bézier) become exact splines: the
  control points are the OBJ's vertices, the knots and weights its own numbers.

## Curved surfaces (optional)

A mesh exported from CAD has its vertices on the original surfaces. With curved
surfaces turned on (`--curves`, or the download menu), obj2cad looks for regions whose
**every vertex lies on one cylinder, cone, sphere or torus within the precision the file
writes it with** (a coordinate written `1.234560` is known to ±0.5e-6; never looser than a
millionth of the model's size), whose faces hug that surface the way a tessellation does,
and whose boundary is lines and circles on it. Each becomes an ACIS surface on a
"Curves" layer, **next to the unchanged mesh**; the parity hash still covers the mesh.
Anything that doesn't pass stays faceted: nothing is approximated to make a surface fit.
Scans are noisier than their stated precision, so they rarely have such regions.

The report lists each surface with the source faces it came from; the harness re-checks
every one of their vertices against the surface read back from the file.

## Automatic choices

OBJ files store neither units nor which way is up, so obj2cad decides both:

- **Up direction** is detected from the geometry: which way the model rests on a flat base,
  or which axis it lies flat along. Only when the shape is ambiguous does it fall back to
  the exporter's convention. Standing a model upright is an exact axis swap.
- **Units** come from the exporter's convention (Blender: meters, SketchUp: inches, …);
  a file that doesn't name its exporter is taken as meters, or as your remembered "house
  unit". Units only label the drawing (`$INSUNITS`); coordinates never change, so a wrong
  guess can't damage geometry. In the app, "Show in" picks the unit sizes and dimensions
  are displayed in; it never changes the file. The file's own unit is shown beneath it
  and can be changed there.

Both can be changed in one click. The command-line tool makes the same decisions by
default, so the same file gives the same drawing (byte for byte) in both;
`obj2cad inspect file.obj` prints them.

## Verification

`tests/harness/parity.py` checks every conversion independently of the Rust code:
its own OBJ reader, a DXF read-back through [ezdxf](https://github.com/mozman/ezdxf)
(with ezdxf's audit) or a DWG read-back through acadrust's reader, a three-way
parity-hash match (Rust source ↔ Python source ↔ read-back), and every element's layer
and color. With `--curves` it also re-checks every recognized surface. CI runs it on
Windows, macOS and Linux for every axis mode and output format.

ACIS output (the surface data inside the drawing) is read back by two independent
readers, ezdxf and acadrust. `obj2cad acis-samples DIR` writes known bodies (plane, box,
cylinder, cone, sphere, torus) for checking in AutoCAD.

The web app has browser tests (`web/e2e`, Playwright) on the built site: every fixture is
opened, downloaded in each format, and must be byte-identical to the command-line output
for the same file; the report the app saves must be the command-line report for that same
download (also for a download without some layers). The parser is also property-tested (`crates/obj2cad-core/tests`) and
fuzzed nightly (`fuzz/`).

## Try it

Command-line binaries for Linux, macOS and Windows are attached to every
[release](https://github.com/cbassuarez/obj2cad/releases/latest). To build from source:

```bash
python tools/vendor/fetch_acadrust.py     # once: the patched DWG writer
cargo build --release -p obj2cad-cli
./target/release/obj2cad convert model.obj
./target/release/obj2cad convert site.zip --curves      # a bundle, with curved surfaces
```

Options: `--format dxf|dxf-binary|dwg`, `--units auto|unitless|mm|cm|m|in|ft` (written as
`$INSUNITS`; coordinates are never scaled), `--default-units` (the house unit),
`--up auto|as-is|y-to-z` (exact axis swap), `--layers objects|groups|materials|single`,
`--keep-loose-points`, `--exclude-layer NAME`, `--mtl file.mtl` (material colors),
`--curves`, `-o out.dxf`, `--report r.json`. Inputs can be files, folders and `.zip`
files; a lone `.obj` brings the `.mtl` and textures it names. `obj2cad` with no
arguments lists everything.

Web app: React + TypeScript, Tailwind v4, shadcn/ui (Radix) controls, Mantine (dropzone,
modal, menu, notifications), Motion, three.js; the same Rust core compiled to WebAssembly in
a Web Worker. One set of CSS tokens themes all of it (a single light "drafting" theme).

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cd web && npm ci && npm run dev
```

Verify everything:

```bash
pip install -r tests/harness/requirements.txt
python tests/harness/parity.py --up auto tests/fixtures
python tests/harness/parity.py --up auto --format dwg tests/fixtures
cd web && npm run build && npx playwright install chromium && npm run e2e
```

## Releases and updates

Commits follow [Conventional Commits](https://www.conventionalcommits.org/). release-please
keeps a release PR open with the changelog and version bump; merging it tags the release,
re-runs all verification, and deploys to GitHub Pages:

- `https://cbassuarez.com/obj2cad/`: latest version. Installable as an app, works
  offline, and shows **"A new version is ready → Reload"** when an update lands. It never
  reloads on its own.
- `https://cbassuarez.com/obj2cad/v/<version>/`: every release stays available,
  unchanged, so any past conversion can be reproduced with the exact engine that made it
  (the version is stored in each file's properties).

Each GitHub Release carries the web bundle, command-line binaries, an SPDX SBOM, SHA-256
checksums and build provenance attestations. With a GitHub App configured
(`RELEASE_APP_ID`, `RELEASE_APP_PRIVATE_KEY` secrets), release PRs run CI like any other. Before merging a release PR, run the
[AutoCAD acceptance checklist](docs/ACCEPTANCE.md).

## Layout

| Path | Purpose |
|---|---|
| `crates/obj2cad-core` | Strict, lossless OBJ and XYZ parsers; bundles; OBJ → CAD model; parity hash; report |
| `crates/obj2cad-curves` | Recognizing cylinders, cones, spheres and tori, strictly |
| `crates/obj2cad-acis` | ACIS B-rep writer (SAB and SAT) |
| `crates/obj2cad-dxf` | Exact DXF R2018 writer (ASCII and binary) |
| `crates/obj2cad-dwg` | DWG writer (acadrust, patched by `tools/vendor`) |
| `crates/obj2cad-cli` | Command-line tool (`convert`, `inspect`, `bench`) |
| `crates/obj2cad-wasm` | WebAssembly bindings used by the web app |
| `web/` | The web app |
| `tests/fixtures` | Hand-built edge cases (`edge/`), bundles (`bundle/`), curved shapes (`curves/`, from `tools/fixtures/curves.py`) and files that must be rejected (`invalid/`) |
| `tests/harness` | Independent parity verification |
| `web/e2e` | Browser tests of the built web app |
| `fuzz` | Coverage-guided fuzzing of the parser and pipeline (nightly Rust) |
| `tools/corpus` | Scripts that fetch the ABC dataset test corpus |
| `tools/dxf-template` | Generator for the DXF document skeleton |

Performance goals and current numbers: [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

## License

MIT. Test data from the [ABC dataset](https://deep-geometry.github.io/abc-dataset/) is MIT
licensed (© 2019 Deep Geometry Processing).
