# Performance

**Goal: usably fast.** A designer drops a big file and has a verified result before they
wonder whether it's working; changing a setting feels immediate.

## Metrics and budgets

Reference file: `obj2cad bench --synthetic 1000`, a 70.3 MB OBJ with 1,000,000 vertices and
1,497,001 faces (mixed quads and triangles, 6-decimal coordinates, like most exporters).

| # | Metric | Budget | Now (2026-09-28, desktop Chrome, Windows 11) |
|---|---|---|---|
| M1 | Browser: drop → verified DXF + 3D preview | ≤ 2.0 s | **1.5–2.0 s** (first load includes fetching the 3D module) |
| M2 | Browser: change the up direction → new DXF | ≤ 1.0 s | **0.85–1.1 s** (at the edge; see Next levers) |
| M3 | Native CLI throughput (parse + convert + hash + write) | ≥ 100 MB/s | **114 MB/s** (616 ms) |
| M4 | Native CLI on GitHub's `ubuntu-latest` (regression guard) | ≤ 1,500 ms | enforced in CI |

M1 and M2 are measured in the real app: drop the file, open "Technical details" and hover
"Time" for every stage. M3/M4 come from `obj2cad bench`.

## Where the time goes (M1, browser)

| Stage | ms | Notes |
|---|---|---|
| Parse | 391 | once per file; the parsed `Session` stays in WebAssembly memory |
| Convert | 16 | layers, colors, entities |
| Parity hash | 456 → 240 on re-run | canonical bytes in Rust, SHA-256 by the browser's native (hardware) digest |
| Write DXF | 274 | 125 MB of text |
| Output SHA-256 | 97 | native digest |
| Preview buffers | 31 | float32, display only |

## What made it fast

- **Parse once per file.** Changing a setting re-runs conversion only (`Session` in
  `crates/obj2cad-wasm`).
- **Native digests in the browser.** SHA-256 in WebAssembly has no hardware
  acceleration; `crypto.subtle` does. The parity hash feeds Rust's canonical byte
  stream to the browser digest, so it matches the CLI exactly (checked on fixtures).
- **Writer:** direct byte output, no `format!`; the source token is copied verbatim when it
  is plain decimal (no re-parse: parsing is correctly rounded and sign-symmetric, so the
  text round-trips by construction, and a debug assertion checks it).
- **Hash:** records in one flat buffer, sorted by offset (no per-face allocation).
- **Convert:** flat vertex → local index table instead of per-mesh hash maps.
- **Parser:** no per-line allocation, `memchr` line splitting, hand-rolled index parsing.
- **Move-out buffers** from WebAssembly (one copy into JS, not clone + copy), transferred
  to the main thread without copying.
- WebAssembly SIMD enabled (`.cargo/config.toml`).

## Next levers, if needed

- M2 is at its budget. About 0.63 s is engine work (write, parity, output digest); the rest
  is main-thread geometry upload and React re-render. Reusing GPU buffers when only the
  orientation changes (a transform, not new geometry) is the first thing to try.
- Up-direction detection costs 15 ms on the 1.5M-face reference (`obj2cad bench`).
- Stream the DXF to disk with the File System Access API instead of holding 125 MB.
- Multi-threaded WebAssembly (needs cross-origin isolation headers, which GitHub Pages
  can't set; would need a service-worker shim).
- Faster float parsing (`fast-float2`) if parse becomes dominant.
