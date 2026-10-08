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
| Parity hash | 456 → 240 on re-run | measured with the browser's digest; now in Rust, after the preview (below) |
| Write DXF | 274 | 125 MB of text |
| Output SHA-256 | 97 | native digest |
| Preview buffers | 31 | float32, display only |

Since 0.6 the order is: parse, convert, **preview** (the model is shown), then parity hash
and write while it is; Download waits for the file. The figures above predate that.

## What made it fast

- **Parse once per file.** Changing a setting re-runs conversion only (`Session` in
  `crates/obj2cad-wasm`).
- **The model before the file.** The preview is built right after conversion and shown
  with a provisional report (no hash, no size yet); the parity hash and the file follow
  while the model is on screen, and Download waits for them ("Writing the DXF…"). The
  loading screen names each step, with progress where it can be counted, and can cancel.
- **Parity hash inside the engine.** Computed from the model the conversion already built
  and streamed straight into SHA-256 (the same bytes as `hash::parity_stream`, so it
  matches the CLI; the harness checks every fixture). It used to go to the browser's
  hardware digest, which is faster at SHA-256 itself, but that meant building the model
  twice, growing WebAssembly memory (which never shrinks) by the whole canonical stream
  (62 MB for 730k faces) and copying it again into JS: slower overall in measurements,
  and double the peak memory on the largest files. Records sort by a 128-bit prefix
  before comparing bytes (native: 150 → 107 ms on 730k faces).
- **The file is assembled on the main thread.** The writer's pieces are transferred from
  the worker as they are (no copy); building one `Blob` from them there took a large
  fraction of a second, and about 40 ms on the main thread.
- **One draw per layer** in the preview, however many colors a layer has (a textured
  model can have hundreds).
- **Writer:** direct byte output, no `format!`; the source token is copied verbatim when it
  is plain decimal (no re-parse: parsing is correctly rounded and sign-symmetric, so the
  text round-trips by construction, and a debug assertion checks it).
- **Hash:** records in one flat buffer, sorted by offset (no per-face allocation).
- **Convert:** flat vertex → local index table instead of per-mesh hash maps.
- **Parser:** no per-line allocation, `memchr` line splitting, hand-rolled index parsing.
- **Move-out buffers** from WebAssembly (one copy into JS, not clone + copy), transferred
  to the main thread without copying.
- WebAssembly SIMD enabled (`.cargo/config.toml`).

### Point clouds

Reference: an OBJ with a 94 MB, 3-million-point colored `.xyz` scan (392 MB of DXF).
Native, before → after: reading the scan 4.7 → 1.0 s, writing 1.2 → 0.6 s, the whole CLI
run 16.1 → 9.5 s; every output byte-identical.

- **One pass over a cloud:** no list of lines, no allocation per line, each number read
  once (the extra columns are kept compactly until their meaning is decided).
- **Plain decimals read fast and exactly:** digits as an integer below 2^53 divided by an
  exact power of ten, one correctly rounded division (Clinger's fast path, which
  `str::parse` takes too); anything else goes to `str::parse`. OBJ numbers too.
- **Writer:** group codes and entity handles written directly (no formatting machinery
  or allocation per entity); coordinate text copied as bytes.
- **Hash:** records sort by their first 32 bytes as integers (a point is decided
  without comparing bytes), and the stream reaches SHA-256 in 64 KB pieces.
- **In the app:** a .zip is no longer unpacked on the page (it froze it for seconds):
  the page reads the archive's directory, and the worker takes each file out with the
  browser's own decompressor. A large cloud next to models loads after them, with a
  spinner in the layers pane; the file is still written once. The written file becomes a
  `Blob` a few megabytes at a time while the page is idle (in one go, a 370 MB file held
  the page for over three seconds). Layer extents come from the engine, so three.js never
  scans vertex buffers, and colors arrive ready for display.

### Memory: large scans in the browser

WebAssembly has 4 GB. Reference: a Matterport export, a 1.7 GB `.xyz` (43 million colored
points) with a textured model (466 textures of 2048²). It used to stop the engine while
parsing (it needed about 5 GB for the cloud alone, and 5.6 GB more for the decoded
textures); it now converts with a peak of 2.55 GB. Outputs are byte-identical to before
(every fixture in every format, and a 2.5-million-point slice of this export).

- **The file stays in the browser.** A file of 64 MB or more isn't copied into the engine;
  it is read from the browser's copy in 4 MB pieces as it is parsed (`bundle::Content`),
  so a cloud's text is never in engine memory.
- **Coordinate text, one byte each.** A plain decimal is kept as its number of decimals and
  rebuilt from its value when written (checked exact when stored); other text is kept as
  written (`coords.rs`). 1.7 GB of text and offsets became 130 MB.
- **Colors as bytes** (`vertex_colors.rs`): a cloud's colors are 3 bytes a point, not an
  optional float triple (16), and vertices without a color take nothing.
- **No doubling.** The reader sizes its arrays once from the first 4 MB; a bundle's files are
  joined by moving the largest one's arrays, not copying them; the point list and the
  preview are sized up front (the preview to the points it shows).
- **Textures sampled once, one at a time,** while loading: each face's color from its
  texture is kept and the image dropped (`Bundle::face_textures`).
- **The parity hash sorts points in batches** of 8 million (splitters from a sample), the
  same bytes as one sort.
- When memory still runs out, the app says the drawing is too large for the browser (the
  allocator notes the failed request) instead of a generic error.

### DWG of a large scan

acadrust (the DWG writer) builds the whole drawing as objects, then the whole objects
section, then the file: for the reference scan that is tens of gigabytes, natively too.
obj2cad's fork (`vendor/acadrust/OBJ2CAD.md`) streams the points of a drawing with more
than a million: each is filled into one reusable POINT and encoded by acadrust's own code,
the AcDbObjects section is compressed and written a page at a time, model space's
BLOCK_HEADER (which lists every point) is written last with its list spliced in as it goes
out, and the handle map keeps one byte per point. The file goes out as it is made but for
its first 256 bytes, written last (the browser puts them back in front).

| Reference scan, DWG | Time | Peak memory | File |
|---|---|---|---|
| CLI (native) | 137 s | 3.3 GB | 1.05 GB |
| Web engine (WebAssembly) | 134 s from adding the files | 2.7 GB | the same size |

Drawings below the threshold are written exactly as before (byte-identical on every
fixture). The streamed layout is read back exactly by acadrust's reader (70,000 points in
the unit tests; a 2.5-million-point slice of the reference through the parity harness).
The reference's objects section is 2.17 GB: past 2 GB, its handle map's offsets are wider
than 32 bits, which acadrust reads; that a file this large opens in AutoCAD is still to be
confirmed.

## Next levers, if needed

- M2 is at its budget. About 0.63 s is engine work (write, parity, output digest); the rest
  is main-thread geometry upload and React re-render. Reusing GPU buffers when only the
  orientation changes (a transform, not new geometry) is the first thing to try.
- Up-direction detection costs 15 ms on the 1.5M-face reference (`obj2cad bench`).
- Stream the DXF to disk with the File System Access API instead of holding 125 MB.
- Multi-threaded WebAssembly (needs cross-origin isolation headers, which GitHub Pages
  can't set; would need a service-worker shim).
- Faster float parsing (`fast-float2`) if parse becomes dominant.
