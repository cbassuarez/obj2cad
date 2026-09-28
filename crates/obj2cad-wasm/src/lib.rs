//! WebAssembly API used by the web app (runs inside a Web Worker).
//!
//! A [`Session`] parses a file once; a settings change only re-runs conversion and
//! writing. Settings and reports cross the boundary as JSON, the output file streams to a
//! JS callback in chunks (it never has to sit in wasm memory), and the preview comes back
//! as typed arrays.

mod preview;

use obj2cad_core::hints::{self, Choices, Hints, UnitsSource};
use obj2cad_core::mtl::Palette;
use obj2cad_core::{
    convert as to_cad, hash, mtl, parse_with_progress, report, CadModel, LayerMode, Meta,
    ObjDocument, Options, ParseError, Units, UpAxis,
};
use preview::Preview;
use serde::Deserialize;
use std::cell::RefCell;
use std::io::Write;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn version() -> String {
    obj2cad_core::VERSION.to_owned()
}

/// True in the module built with the DWG writer.
#[wasm_bindgen]
pub fn has_dwg() -> bool {
    cfg!(feature = "dwg")
}

// ---------------------------------------------------------------- crashes

thread_local! {
    static PANIC_REPORTER: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

/// `f(message)` is called if the engine panics. The instance is unusable afterwards; the
/// web app shows the message and starts a fresh worker.
#[wasm_bindgen]
pub fn on_panic(f: js_sys::Function) {
    PANIC_REPORTER.with(|r| *r.borrow_mut() = Some(f));
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        PANIC_REPORTER.with(|r| {
            if let Some(f) = r.borrow().as_ref() {
                let _ = f.call1(&JsValue::NULL, &JsValue::from_str(&message));
            }
        });
    }));
}

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Format {
    #[default]
    Dxf,
    DxfBinary,
    Dwg,
}

impl Format {
    fn id(self) -> &'static str {
        match self {
            Format::Dxf => obj2cad_dxf::Format::Ascii.id(),
            Format::DxfBinary => obj2cad_dxf::Format::Binary.id(),
            Format::Dwg => "dwg-r2018",
        }
    }
}

/// Everything the user can set. `None` means "decide from the file".
#[derive(Debug, Deserialize)]
#[serde(default)]
struct Settings {
    units: Option<Units>,
    /// The team's unit for files that don't state one.
    default_units: Option<Units>,
    up_axis: Option<UpAxis>,
    layer_mode: LayerMode,
    keep_loose_points: bool,
    exclude_layers: Vec<String>,
    format: Format,
    /// The source file's modification time (the drawing's creation date).
    created_unix: Option<f64>,
    /// Record the source file name in the drawing's properties.
    include_name: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            units: None,
            default_units: None,
            up_axis: None,
            layer_mode: LayerMode::Objects,
            keep_loose_points: false,
            exclude_layers: Vec::new(),
            format: Format::Dxf,
            created_unix: None,
            include_name: true,
        }
    }
}

fn settings_from(json: &str) -> Result<Settings, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("settings: {e}")))
}

// ---------------------------------------------------------------- session

/// One loaded file: parsed once, converted as often as settings change.
#[wasm_bindgen]
pub struct Session {
    doc: ObjDocument,
    palette: Option<Palette>,
    name: String,
    stem: String,
    source_len: u64,
    source_sha: String,
    parse_ms: f64,
    hints: Hints,
}

/// A parse failure as a plain JS object `{kind, line, message, issues, truncated}`.
fn parse_error(e: &ParseError) -> JsValue {
    let json = serde_json::json!({
        "kind": e.kind, "line": e.line, "message": e.message, "issues": e.issues(), "truncated": e.truncated,
    });
    js_sys::JSON::parse(&json.to_string()).unwrap_or_else(|_| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
impl Session {
    /// Parse `obj`. `progress(done, total)` is called every few megabytes. Throws a plain
    /// object `{kind, line, message, issues, truncated}` when the file can't be read
    /// without guessing. `source_sha256` comes from the browser's native digest.
    #[wasm_bindgen(constructor)]
    pub fn new(
        obj: &[u8],
        name: String,
        source_sha256: String,
        progress: Option<js_sys::Function>,
    ) -> Result<Session, JsValue> {
        let t = now();
        let doc = parse_with_progress(obj, |done, total| {
            if let Some(f) = &progress {
                let _ = f.call2(
                    &JsValue::NULL,
                    &JsValue::from_f64(done as f64),
                    &JsValue::from_f64(total as f64),
                );
            }
        })
        .map_err(|e| parse_error(&e))?;
        let parse_ms = now() - t;
        let hints = hints::hints(&doc);
        let stem = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(s, _)| s)
            .to_owned();
        Ok(Session {
            doc,
            palette: None,
            stem,
            name,
            source_len: obj.len() as u64,
            source_sha: source_sha256,
            parse_ms,
            hints,
        })
    }

    /// Attach (or replace) the material library.
    pub fn set_mtl(&mut self, mtl_bytes: &[u8]) {
        self.palette = Some(mtl::parse(mtl_bytes));
    }

    /// What was found in the file and what would be chosen automatically, as JSON.
    pub fn inspect(&self) -> String {
        let d = &self.doc;
        serde_json::json!({
            "vertices": d.positions.len(),
            "faces": d.faces.len(),
            "lines": d.lines.len(),
            "points": d.points.len(),
            "objects": d.objects.len(),
            "groups": d.groups.len(),
            "materials": d.materials,
            "mtllibs": d.mtllibs,
            "hints": self.hints,
            "parse_ms": self.parse_ms,
        })
        .to_string()
    }

    fn options(&self, s: &Settings) -> Options {
        let choices = Choices {
            units: s.units,
            default_units: s.default_units,
            up_axis: s.up_axis,
        };
        let (units, up_axis) = hints::resolve(&self.hints, &choices);
        Options {
            units,
            up_axis,
            layer_mode: s.layer_mode,
            default_layer: self.stem.clone(),
            keep_loose_points: s.keep_loose_points,
            exclude_layers: s.exclude_layers.clone(),
        }
    }

    fn model(&self, s: &Settings) -> CadModel<'_> {
        to_cad(&self.doc, self.palette.as_ref(), self.options(s))
    }

    /// Step 1: the canonical bytes whose SHA-256 is the parity hash. The caller digests
    /// them natively. Only settings that move geometry change it (up axis, loose points,
    /// excluded layers), so callers can cache it across the others.
    pub fn parity_stream(&self, settings: &str) -> Result<Vec<u8>, JsError> {
        Ok(hash::parity_stream(&self.model(&settings_from(settings)?)))
    }

    /// Step 2: convert and write. The file goes to `sink(chunk: Uint8Array)` in pieces of
    /// about 1 MB. The report's `output.sha256` is left empty (hashed on demand).
    pub fn convert(
        &self,
        settings: &str,
        parity: &str,
        want_preview: bool,
        sink: &js_sys::Function,
    ) -> Result<Conversion, JsError> {
        let s = settings_from(settings)?;
        let t0 = now();
        let model = self.model(&s);
        let t1 = now();

        let exact = if model.omissions.is_partial() {
            "partial"
        } else {
            "exact"
        };
        let mut props = vec![
            ("obj2cad.version", obj2cad_core::VERSION),
            ("obj2cad.source_sha256", self.source_sha.as_str()),
            ("obj2cad.parity_hash", parity),
            ("obj2cad.parity", exact),
        ];
        if s.include_name {
            props.push(("obj2cad.source_name", self.name.as_str()));
        }
        let meta = Meta {
            properties: &props,
            fingerprint_seed: &self.source_sha,
            created_unix: s.created_unix,
        };
        let mut out = JsSink {
            f: sink,
            written: 0,
        };
        match s.format {
            Format::Dxf => {
                obj2cad_dxf::write_to(&model, &meta, obj2cad_dxf::Format::Ascii, &mut out)
            }
            Format::DxfBinary => {
                obj2cad_dxf::write_to(&model, &meta, obj2cad_dxf::Format::Binary, &mut out)
            }
            #[cfg(feature = "dwg")]
            Format::Dwg => {
                let bytes =
                    obj2cad_dwg::write(&model, &meta).map_err(|e| JsError::new(&e.to_string()))?;
                bytes
                    .chunks(1 << 20)
                    .try_for_each(|c| out.write_all(c))
                    .map(|()| bytes.len() as u64)
            }
            #[cfg(not(feature = "dwg"))]
            Format::Dwg => {
                return Err(JsError::new(
                    "this engine module has no DWG writer (load the DWG module)",
                ))
            }
        }
        .map_err(|e| JsError::new(&format!("writing the file: {e}")))?;
        let t2 = now();

        let rep = report::build(
            &model,
            &report::Source {
                name: &self.name,
                len: self.source_len,
                sha256: &self.source_sha,
            },
            parity,
            report::Written {
                format: s.format.id(),
                bytes: out.written,
                sha256: None,
            },
        );
        let report = serde_json::to_string(&rep).map_err(|e| JsError::new(&e.to_string()))?;
        let t3 = now();
        let preview = want_preview.then(|| Preview::build(&model));
        let t4 = now();

        // What was chosen, and where it came from, for the dock.
        let units_from = if s.units.is_some() {
            "chosen"
        } else {
            match self.hints.units_source {
                UnitsSource::Exporter => "file",
                _ if s.default_units.is_some() => "default",
                UnitsSource::Size => "size",
                UnitsSource::None => "none",
            }
        };
        let decisions = serde_json::json!({
            "units": model.options.units,
            "units_from": units_from,
            "up_axis": model.options.up_axis,
            "up_from": if s.up_axis.is_some() { "chosen" } else { "detected" },
            "detected_units": hints::resolve(&self.hints, &Choices { units: None, default_units: s.default_units, up_axis: None }).0,
            "detected_up_axis": self.hints.up_axis,
        })
        .to_string();
        let timings = serde_json::json!({
            "parse_ms": self.parse_ms, "convert_ms": t1 - t0, "write_ms": t2 - t1, "report_ms": t3 - t2, "preview_ms": t4 - t3,
        })
        .to_string();
        Ok(Conversion {
            report,
            decisions,
            timings,
            preview,
        })
    }
}

/// Streams the output file to JS in the chunks the writer produces.
struct JsSink<'a> {
    f: &'a js_sys::Function,
    written: u64,
}

impl Write for JsSink<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let chunk = js_sys::Uint8Array::from(buf);
        self.f
            .call1(&JsValue::NULL, &chunk)
            .map_err(|_| std::io::Error::other("the output callback failed"))?;
        self.written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A finished conversion. Each `take_*` moves its preview buffer out (one copy into JS);
/// they are empty when no preview was requested or the model can't be displayed.
#[wasm_bindgen]
pub struct Conversion {
    report: String,
    decisions: String,
    timings: String,
    preview: Option<Preview>,
}

macro_rules! take {
    ($($(#[$doc:meta])* $name:ident: $field:ident -> $ty:ty;)*) => {
        #[wasm_bindgen]
        impl Conversion {
            $(
                $(#[$doc])*
                pub fn $name(&mut self) -> $ty {
                    self.preview.as_mut().map(|p| std::mem::take(&mut p.$field)).unwrap_or_default()
                }
            )*
        }
    };
}

take! {
    /// xyz float32 per preview vertex.
    take_positions: positions -> Vec<f32>;
    /// rgb (0–255) per preview vertex.
    take_colors: colors -> Vec<u8>;
    take_indices: indices -> Vec<u32>;
    /// True polygon edges (index pairs), not the display triangles.
    take_edges: edges -> Vec<u32>;
    /// Per mesh: `[layer, index_start, index_count, edge_start, edge_count]`.
    take_groups: groups -> Vec<u32>;
    take_lines: lines -> Vec<f32>;
    take_line_colors: line_colors -> Vec<u8>;
    /// Per layer: `[layer, vertex_start, vertex_count]`.
    take_line_groups: line_groups -> Vec<u32>;
    take_points: points -> Vec<f32>;
    take_point_colors: point_colors -> Vec<u8>;
    /// Per layer: `[layer, point_start, point_count]`.
    take_point_groups: point_groups -> Vec<u32>;
}

#[wasm_bindgen]
impl Conversion {
    /// Report JSON (see `obj2cad_core::report`).
    pub fn report(&self) -> String {
        self.report.clone()
    }
    /// Units and up direction as resolved, and where each came from, as JSON.
    pub fn decisions(&self) -> String {
        self.decisions.clone()
    }
    /// Per-stage milliseconds, as JSON.
    pub fn timings(&self) -> String {
        self.timings.clone()
    }
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
    /// False when the model's extent is too large to display in float32.
    pub fn preview_available(&self) -> bool {
        self.preview.as_ref().is_some_and(|p| p.available)
    }
    /// Where the preview's origin is in drawing coordinates.
    pub fn origin(&self) -> Vec<f64> {
        self.preview
            .as_ref()
            .map_or_else(Vec::new, |p| p.origin.to_vec())
    }
}

fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        performance_now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}
