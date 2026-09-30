//! WebAssembly API used by the web app (runs inside a Web Worker).
//!
//! A [`Session`] parses a file once; a settings change only re-runs conversion and
//! writing. Settings and reports cross the boundary as JSON, the output file streams to a
//! JS callback in chunks (it never has to sit in wasm memory), and the preview comes back
//! as typed arrays.

mod preview;
mod stream;

use obj2cad_core::bundle::{self, Bundle, InputFile};
use obj2cad_core::hints::{self, Choices, Hints, UnitsSource};
use obj2cad_core::texture::Texture;
use obj2cad_core::{
    convert_with, hash, report, CadModel, LayerMode, Materials, Meta, Options, ParseError, Units,
    UpAxis,
};
use preview::Preview;
use serde::Deserialize;
use std::cell::RefCell;
use std::io::Write;
use stream::Stream;
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
    /// Also write recognized curved surfaces.
    curves: bool,
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
            curves: false,
        }
    }
}

fn settings_from(json: &str) -> Result<Settings, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("settings: {e}")))
}

// ---------------------------------------------------------------- session

/// One loaded bundle (a model, or models, clouds, materials and textures): parsed once,
/// converted as often as settings change.
#[wasm_bindgen]
pub struct Session {
    /// Files added but not loaded yet.
    pending: Vec<(String, Vec<u8>, String, Option<f64>)>,
    bundle: Option<Bundle>,
    parse_ms: f64,
    hints: Option<Hints>,
}

/// A parse failure as a plain JS object `{file, kind, line, message, issues, truncated}`.
fn parse_error(file: &str, e: &ParseError) -> JsValue {
    let json = serde_json::json!({
        "file": file, "kind": e.kind, "line": e.line, "message": e.message, "issues": e.issues(), "truncated": e.truncated,
    });
    js_sys::JSON::parse(&json.to_string()).unwrap_or_else(|_| JsValue::from_str(&e.to_string()))
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session {
            pending: Vec::new(),
            bundle: None,
            parse_ms: 0.0,
            hints: None,
        }
    }

    /// Add a file (`path` may include folders; `sha256` from the browser's native digest,
    /// or empty to give it after loading with `set_hashes`; `modified` in Unix seconds,
    /// or a negative number when unknown).
    pub fn add_file(&mut self, path: String, bytes: Vec<u8>, sha256: String, modified: f64) {
        let modified = (modified >= 0.0).then_some(modified);
        self.pending.push((path, bytes, sha256, modified));
    }

    /// Read everything added into one drawing. `name` names it when it holds several
    /// models or clouds (a zip's or folder's name). Throws a plain object `{file, kind,
    /// line, message, issues, truncated}` when a file can't be read without guessing.
    ///
    /// `progress(done, total, file, cloud, count, points, colors, origin)` is called every
    /// few megabytes: bytes of the models and clouds read of their total, the file being
    /// read, whether it is a point cloud, how many positions it has so far, and the points
    /// read since the last call, to show while reading (thinned, xyz relative to `origin`,
    /// colors in linear light; empty for small files).
    pub fn load(
        &mut self,
        name: String,
        progress: Option<js_sys::Function>,
    ) -> Result<(), JsValue> {
        let t = now();
        let pending = std::mem::take(&mut self.pending);
        let files = pending
            .iter()
            .map(|(path, bytes, sha, modified)| InputFile {
                path: path.clone(),
                bytes,
                sha256: Some(sha.clone()),
                modified: *modified,
            })
            .collect();
        let mut stream = Stream::new(stream::BUDGET);
        let bundle = bundle::load(files, &name, |l| {
            if let Some(f) = &progress {
                stream.take(l);
                let args = js_sys::Array::new();
                args.push(&JsValue::from_f64(l.done as f64));
                args.push(&JsValue::from_f64(l.total as f64));
                args.push(&JsValue::from_str(l.file));
                args.push(&JsValue::from_bool(l.cloud));
                args.push(&JsValue::from_f64(l.partial.positions.len() as f64));
                args.push(&js_sys::Float32Array::from(&stream.points[..]));
                args.push(&js_sys::Uint8Array::from(&stream.colors[..]));
                args.push(&js_sys::Float64Array::from(
                    &stream.origin.unwrap_or_default()[..],
                ));
                let _ = f.apply(&JsValue::NULL, &args);
            }
        })
        .map_err(|e| parse_error(&e.file, &e.error))?;
        self.parse_ms = now() - t;
        self.hints = Some(hints::hints(&bundle.doc));
        self.bundle = Some(bundle);
        Ok(())
    }

    /// The SHA-256 of each file added, in the order they were added, when they were added
    /// without one (so the browser can hash them while the engine parses).
    pub fn set_hashes(&mut self, sha256: Vec<String>) -> Result<(), JsError> {
        let b = self
            .bundle
            .as_mut()
            .ok_or_else(|| JsError::new("no file is loaded"))?;
        b.set_hashes(&sha256);
        Ok(())
    }

    fn loaded(&self) -> Result<(&Bundle, &Hints), JsError> {
        match (&self.bundle, &self.hints) {
            (Some(b), Some(h)) => Ok((b, h)),
            _ => Err(JsError::new("nothing is loaded")),
        }
    }

    /// What was found and what would be chosen automatically, as JSON.
    pub fn inspect(&self) -> Result<String, JsError> {
        let (b, h) = self.loaded()?;
        let d = &b.doc;
        Ok(serde_json::json!({
            "name": b.name,
            "vertices": d.positions.len(),
            "faces": d.faces.len(),
            "lines": d.lines.len(),
            "points": d.points.len(),
            "objects": d.objects.len(),
            "groups": d.groups.len(),
            "materials": d.materials,
            "mtllibs": d.mtllibs,
            "files": b.files,
            "hints": h,
            "parse_ms": self.parse_ms,
        })
        .to_string())
    }

    fn options(&self, s: &Settings) -> Result<Options, JsError> {
        let (b, h) = self.loaded()?;
        let choices = Choices {
            units: s.units,
            default_units: s.default_units,
            up_axis: s.up_axis,
        };
        let (units, up_axis) = hints::resolve(h, &choices);
        Ok(Options {
            units,
            up_axis,
            layer_mode: s.layer_mode,
            default_layer: b.stem.clone(),
            keep_loose_points: s.keep_loose_points,
            exclude_layers: s.exclude_layers.clone(),
            curves: s.curves,
        })
    }

    fn model(&self, s: &Settings) -> Result<CadModel<'_>, JsError> {
        let (b, _) = self.loaded()?;
        let textures = b
            .textures
            .iter()
            .map(|(m, t)| {
                (
                    m.as_str(),
                    Texture {
                        image: &b.images[t.image],
                        map: &t.map,
                    },
                )
            })
            .collect();
        let materials = Materials {
            palette: b.palette.as_ref(),
            textures,
        };
        Ok(convert_with(&b.doc, &materials, self.options(s)?))
    }

    /// The model to show, with a provisional report (no parity hash, no output size), and
    /// no file: for showing part of a bundle while the rest of it loads. The file is only
    /// ever written by `convert`, once, from everything.
    pub fn preview(&self, settings: &str) -> Result<Conversion, JsError> {
        let s = settings_from(settings)?;
        let t0 = now();
        let model = self.model(&s)?;
        let (b, h) = self.loaded()?;
        let report = report::build(
            &model,
            &report::Source {
                name: &b.name,
                len: b.source_len,
                sha256: &b.source_sha256,
                files: &b.files,
            },
            "",
            report::Written {
                format: s.format.id(),
                bytes: 0,
                sha256: None,
            },
        );
        let preview = Preview::build(&model);
        let timings =
            serde_json::json!({ "parse_ms": self.parse_ms, "preview_ms": now() - t0 }).to_string();
        Ok(Conversion {
            parity: String::new(),
            report: serde_json::to_string(&report).map_err(|e| JsError::new(&e.to_string()))?,
            decisions: decisions(&s, &model, h),
            timings,
            preview: Some(preview),
        })
    }

    /// Convert and write. The file goes to `sink(chunk: Uint8Array)` in pieces of about
    /// 1 MB. The report's `output.sha256` is left empty (hashed on demand).
    ///
    /// `parity` is the parity hash when the caller already has it for these settings, or
    /// empty to compute it here (read it back with `Conversion::parity`). Only settings
    /// that move geometry change it (up axis, loose points, excluded layers), so callers
    /// can cache it across the others. It covers the mesh, not the curved surfaces
    /// written next to it.
    ///
    /// The model can be shown before the file exists: with `early`, the preview is built
    /// first and `early(conversion)` receives it with a provisional report (no parity
    /// hash, no output size) before the hash and the file are made; the returned
    /// conversion then has no preview.
    ///
    /// `stage(name)` is called as each step starts: `"curves"`, `"preview"`, `"hash"`,
    /// `"write"`. While a DXF is written, `progress(done, total)` counts the elements
    /// written of all there are (see `obj2cad_dxf::work`), with each piece of the file.
    #[allow(clippy::too_many_arguments)]
    pub fn convert(
        &self,
        settings: &str,
        parity: &str,
        want_preview: bool,
        sink: &js_sys::Function,
        stage: Option<js_sys::Function>,
        early: Option<js_sys::Function>,
        progress: Option<js_sys::Function>,
    ) -> Result<Conversion, JsError> {
        let stage = |name: &str| {
            if let Some(f) = &stage {
                let _ = f.call1(&JsValue::NULL, &JsValue::from_str(name));
            }
        };
        let s = settings_from(settings)?;
        let t0 = now();
        let mut model = self.model(&s)?;
        // Curved surfaces sit next to the mesh (they don't change it, or its hash).
        let curves = model.options.curves.then(|| {
            stage("curves");
            obj2cad_curves::add_to(&mut model).to_string()
        });
        let (b, h) = self.loaded()?;
        let decisions = decisions(&s, &model, h);
        let source = report::Source {
            name: &b.name,
            len: b.source_len,
            sha256: &b.source_sha256,
            files: &b.files,
        };
        let t1 = now();

        let mut preview = want_preview.then(|| {
            stage("preview");
            Preview::build(&model)
        });
        let t_preview = now();
        if let (Some(f), true) = (&early, preview.is_some()) {
            let provisional = report::build(
                &model,
                &source,
                "",
                report::Written {
                    format: s.format.id(),
                    bytes: 0,
                    sha256: None,
                },
            );
            let first = Conversion {
                parity: String::new(),
                report: serde_json::to_string(&provisional)
                    .map_err(|e| JsError::new(&e.to_string()))?,
                decisions: decisions.clone(),
                timings: "{}".into(),
                preview: preview.take(),
            };
            let _ = f.call1(&JsValue::NULL, &JsValue::from(first));
        }

        let t_hash0 = now();
        let parity = if parity.is_empty() {
            stage("hash");
            hash::parity_hash(&model)
        } else {
            parity.to_owned()
        };
        let parity = parity.as_str();
        let t_hash = now();

        stage("write");
        let exact = if model.omissions.is_partial() {
            "partial"
        } else {
            "exact"
        };
        let mut props = vec![
            ("obj2cad.version", obj2cad_core::VERSION),
            ("obj2cad.source_sha256", b.source_sha256.as_str()),
            ("obj2cad.parity_hash", parity),
            ("obj2cad.parity", exact),
        ];
        if s.include_name {
            props.push(("obj2cad.source_name", b.name.as_str()));
        }
        let curves_label = curves.map(|n| obj2cad_curves::label(&n));
        if let Some(c) = &curves_label {
            props.push(("obj2cad.curves", c.as_str()));
        }
        let meta = Meta {
            properties: &props,
            fingerprint_seed: &b.source_sha256,
            created_unix: b.modified.or(s.created_unix),
        };
        let mut out = JsSink {
            f: sink,
            written: 0,
        };
        let mut report_written = |done: u64, total: u64| {
            if let Some(f) = &progress {
                let _ = f.call2(
                    &JsValue::NULL,
                    &JsValue::from_f64(done as f64),
                    &JsValue::from_f64(total as f64),
                );
            }
        };
        match s.format {
            Format::Dxf => obj2cad_dxf::write_to_with_progress(
                &model,
                &meta,
                obj2cad_dxf::Format::Ascii,
                &mut out,
                &mut report_written,
            ),
            Format::DxfBinary => obj2cad_dxf::write_to_with_progress(
                &model,
                &meta,
                obj2cad_dxf::Format::Binary,
                &mut out,
                &mut report_written,
            ),
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
            &source,
            parity,
            report::Written {
                format: s.format.id(),
                bytes: out.written,
                sha256: None,
            },
        );
        let report = serde_json::to_string(&rep).map_err(|e| JsError::new(&e.to_string()))?;
        let t3 = now();
        let timings = serde_json::json!({
            "parse_ms": self.parse_ms, "convert_ms": t1 - t0, "preview_ms": t_preview - t1,
            "hash_ms": t_hash - t_hash0, "write_ms": t2 - t_hash, "report_ms": t3 - t2,
        })
        .to_string();
        Ok(Conversion {
            parity: parity.to_owned(),
            report,
            decisions,
            timings,
            preview,
        })
    }
}

/// Units and up direction as resolved, and where each came from, as JSON (for the app).
fn decisions(s: &Settings, model: &CadModel, h: &Hints) -> String {
    let units_from = if s.units.is_some() {
        "chosen"
    } else {
        match h.units_source {
            UnitsSource::Exporter => "file",
            _ if s.default_units.is_some() => "default",
            UnitsSource::Assumed => "assumed",
        }
    };
    serde_json::json!({
        "units": model.options.units,
        "units_from": units_from,
        "up_axis": model.options.up_axis,
        "up_from": if s.up_axis.is_some() { "chosen" } else { "detected" },
        "detected_units": hints::resolve(h, &Choices { units: None, default_units: s.default_units, up_axis: None }).0,
        "detected_up_axis": h.up_axis,
    })
    .to_string()
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
    parity: String,
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
    /// Per layer: `[layer, index_start, index_count, edge_start, edge_count]`.
    take_groups: groups -> Vec<u32>;
    take_lines: lines -> Vec<f32>;
    take_line_colors: line_colors -> Vec<u8>;
    /// Per layer: `[layer, vertex_start, vertex_count]`.
    take_line_groups: line_groups -> Vec<u32>;
    take_points: points -> Vec<f32>;
    take_point_colors: point_colors -> Vec<u8>;
    /// Per layer: `[layer, point_start, point_count]`.
    take_point_groups: point_groups -> Vec<u32>;
    /// Per layer shown: `[layer, min x, y, z, max x, y, z]` in display coordinates.
    take_layer_bounds: layer_bounds -> Vec<f32>;
}

#[wasm_bindgen]
impl Conversion {
    /// The parity hash written into the file (computed, or the one passed in).
    pub fn parity(&self) -> String {
        self.parity.clone()
    }
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
    /// 1, or n when a large cloud is shown as every n-th point.
    pub fn point_stride(&self) -> u32 {
        self.preview.as_ref().map_or(1, |p| p.point_stride)
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
