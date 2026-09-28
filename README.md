# obj2cad

Convert Wavefront OBJ to DWG/DXF **without changing a single bit of geometry**.

**Use it: https://cbassuarez.github.io/obj2cad/** (runs entirely in your browser; files are
never uploaded; installable and works offline).

> Status: pre-release (see [the plan](docs/PLAN.md)). DXF output works and is verified;
> DWG and curve reconstruction are next.

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

## Verification

`tests/harness/parity.py` checks every conversion independently of the Rust code:
its own OBJ reader, a DXF read-back through [ezdxf](https://github.com/mozman/ezdxf),
ezdxf's audit, and a three-way parity-hash match (Rust source ↔ Python source ↔ DXF
read-back). CI runs it on Windows, macOS and Linux for both axis modes.

## Try it

```bash
cargo build --release -p obj2cad-cli
./target/release/obj2cad convert model.obj --units mm --up y-to-z
```

Options: `--units unitless|mm|cm|m|in|ft` (written as `$INSUNITS`; coordinates are never
scaled), `--up as-is|y-to-z` (exact axis swap), `--mtl file.mtl` (material colors),
`-o out.dxf`, `--report r.json`.

Web app: React + TypeScript, Tailwind v4, shadcn/ui (Radix) controls, Mantine (dropzone,
modal, menu, notifications), Motion, three.js; the same Rust core compiled to WebAssembly in
a Web Worker. One set of CSS tokens themes all of it (dark "studio", light "drafting").

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cd web && npm ci && npm run dev
```

Verify everything:

```bash
pip install -r tests/harness/requirements.txt
python tests/harness/parity.py --up as-is tests/fixtures
```

## Releases and updates

Commits follow [Conventional Commits](https://www.conventionalcommits.org/). release-please
keeps a release PR open with the changelog and version bump; merging it tags the release,
re-runs all verification, and deploys to GitHub Pages:

- `https://cbassuarez.github.io/obj2cad/`: latest version. Installable as an app, works
  offline, and shows **"A new version is ready → Reload"** when an update lands. It never
  reloads on its own.
- `https://cbassuarez.github.io/obj2cad/v/<version>/`: every release stays available,
  unchanged, so any past conversion can be reproduced with the exact engine that made it
  (the version is stored in each file's properties).

Each GitHub Release carries the web bundle, an SPDX SBOM, SHA-256 checksums and a build
provenance attestation. Before merging a release PR, run the
[AutoCAD acceptance checklist](docs/ACCEPTANCE.md).

## Layout

| Path | Purpose |
|---|---|
| `crates/obj2cad-core` | Strict, lossless OBJ parser; OBJ → CAD model; parity hash; report |
| `crates/obj2cad-dxf` | Exact DXF R2018 writer |
| `crates/obj2cad-cli` | Command-line tool (`convert`, `bench`) |
| `crates/obj2cad-wasm` | WebAssembly bindings used by the web app |
| `web/` | The web app |
| `tests/fixtures` | Hand-built edge cases (`edge/`) and files that must be rejected (`invalid/`) |
| `tests/harness` | Independent parity verification |
| `tools/corpus` | Scripts that fetch the ABC dataset test corpus |
| `tools/dxf-template` | Generator for the DXF document skeleton |

Performance goals and current numbers: [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

## License

MIT. Test data from the [ABC dataset](https://deep-geometry.github.io/abc-dataset/) is MIT
licensed (© 2019 Deep Geometry Processing).
