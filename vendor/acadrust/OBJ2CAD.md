# obj2cad's fork of acadrust

This is [acadrust](https://github.com/hakanaktt/acadrust) 0.5.5 as published on crates.io
(crate SHA-256 `6298485f7afd00af7880f285f01ab387143a1fbb20c42f9048830c95b19dda5d`), with
the changes below. obj2cad uses it only to write DWG. It is MPL-2.0 (see `LICENSE`); the
modified files stay under that license, and the changes are marked `obj2cad:` in the
source where they aren't self-evident. The workspace uses it through `[patch.crates-io]` in the root `Cargo.toml`.

## Changes

1. **Negative zero** (`src/io/dwg/dwg_stream_writers/bit_writer.rs`). Bit-doubles were
   compared with `== 0.0` / `== 1.0`, and IEEE-754 has `-0.0 == 0.0`, so every negative
   zero was written as positive zero. Both the short codes and
   `write_bit_double_with_default` now compare bit patterns.
2. **Drawing properties** (`src/io/dwg/dwg_writer.rs`, `build_summary_info`). The
   SummaryInfo section was always written empty, losing DWGPROPS (obj2cad records
   `obj2cad.parity` and friends there). It now writes the document's summary and custom
   properties, in the layout acadrust's reader parses.
3. **Streaming model-space entities** (`DwgWriter::write_streaming`). A drawing can have
   entities that are never held as acadrust objects: a large point cloud's tens of
   millions of POINTs. They are encoded one at a time as the file is written, the
   AcDbObjects section goes out a page at a time, and the handle map stores them as one
   byte each. See `src/io/dwg/streaming.rs`; the other changes it needs are in
   `dwg_writer.rs` (`write_streaming`, the AC18 path), `file_headers/file_header_ac18.rs`
   (sections written in pieces), `dwg_stream_writers/object_writer/` (`streamed.rs`, the
   deferred BLOCK_HEADER, records left out of the duplicate set and handle map) and
   `dwg_stream_writers/handle_writer.rs` (sorted input, offsets past 2 GB).

Changes 1 and 2 should go upstream; until 0.5.5 was forked they were applied by a
fetch-and-patch script.
