//! WebAssembly API used by the web app (runs inside a Web Worker).
//!
//! A [`Session`] parses a file once and keeps the result, so changing a setting only
//! re-runs conversion and writing. Structured results cross the boundary as JSON strings
//! and typed arrays, which keeps the binding layer thin and the JS side easy to test.

use obj2cad_core::mtl::Palette;
use obj2cad_core::{
    convert as to_cad, hash, hints, mtl, parse, report, ObjDocument, Options, Units, UpAxis,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn version() -> String {
    obj2cad_core::VERSION.to_owned()
}

fn units_from(s: &str) -> Result<Units, JsError> {
    Ok(match s {
        "unitless" => Units::Unitless,
        "mm" => Units::Millimeters,
        "cm" => Units::Centimeters,
        "m" => Units::Meters,
        "in" => Units::Inches,
        "ft" => Units::Feet,
        _ => return Err(JsError::new(&format!("unknown unit `{s}`"))),
    })
}

fn up_from(s: &str) -> Result<UpAxis, JsError> {
    Ok(match s {
        "as-is" => UpAxis::AsIs,
        "y-to-z" => UpAxis::YUpToZUp,
        _ => return Err(JsError::new(&format!("unknown axis mode `{s}`"))),
    })
}

/// One loaded file: parsed once, converted as often as settings change.
#[wasm_bindgen]
pub struct Session {
    doc: ObjDocument,
    palette: Option<Palette>,
    name: String,
    source_len: u64,
    source_sha: String,
    parse_ms: f64,
}

#[wasm_bindgen]
impl Session {
    /// Parse `obj` (and optional MTL). `source_sha256` is computed by the caller with the
    /// browser's native digest. Fails with a line-numbered message on ambiguous input.
    #[wasm_bindgen(constructor)]
    pub fn new(
        obj: &[u8],
        mtl_bytes: Option<Vec<u8>>,
        name: String,
        source_sha256: String,
    ) -> Result<Session, JsError> {
        let t = now();
        let doc = parse(obj).map_err(|e| JsError::new(&e.to_string()))?;
        let parse_ms = now() - t;
        Ok(Session {
            doc,
            palette: mtl_bytes.map(|b| mtl::parse(&b)),
            name,
            source_len: obj.len() as u64,
            source_sha: source_sha256,
            parse_ms,
        })
    }

    /// Attach (or replace) the material library after loading.
    pub fn set_mtl(&mut self, mtl_bytes: &[u8]) {
        self.palette = Some(mtl::parse(mtl_bytes));
    }

    /// Counts and settings suggestions for the setup prompt, as JSON.
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
            "header_comments": d.header_comments,
            "hints": hints::hints(d),
            "parse_ms": self.parse_ms,
        })
        .to_string()
    }

    /// Step 1 of a conversion: the canonical bytes whose SHA-256 is the parity hash
    /// (`obj2cad_core::hash::parity_stream`). The caller digests them natively and passes
    /// the hex digest to [`Session::convert`].
    pub fn parity_stream(&self, units: &str, up: &str) -> Result<Vec<u8>, JsError> {
        let options = Options {
            units: units_from(units)?,
            up_axis: up_from(up)?,
        };
        let model = to_cad(&self.doc, self.palette.as_ref(), options);
        Ok(hash::parity_stream(&model))
    }

    /// Step 2: convert and write with the given settings and parity hash. Cheap to call
    /// again: the file is not re-parsed. The report's `output.sha256` is left empty for
    /// the caller to fill with a native digest.
    pub fn convert(&self, units: &str, up: &str, parity: String) -> Result<Conversion, JsError> {
        let options = Options {
            units: units_from(units)?,
            up_axis: up_from(up)?,
        };
        let t0 = now();
        let model = to_cad(&self.doc, self.palette.as_ref(), options);
        let t1 = now();
        let t2 = t1;
        let props = [
            ("obj2cad.version", obj2cad_core::VERSION),
            ("obj2cad.source_sha256", self.source_sha.as_str()),
            ("obj2cad.parity_hash", parity.as_str()),
            ("obj2cad.parity", "exact"),
            ("obj2cad.source_name", self.name.as_str()),
        ];
        let dxf = obj2cad_dxf::write(&model, &props, &self.source_sha);
        let t3 = now();
        let rep = report::build(
            &model,
            &report::Source {
                name: &self.name,
                len: self.source_len,
                sha256: &self.source_sha,
            },
            &parity,
            "dxf-r2018",
            &dxf,
            Some(""),
        );
        let report = serde_json::to_string(&rep).map_err(|e| JsError::new(&e.to_string()))?;
        let t4 = now();
        let preview = Preview::build(&model);
        let t5 = now();
        let timings = serde_json::json!({
            "parse_ms": self.parse_ms, "convert_ms": t1 - t0,
            "write_ms": t3 - t2, "report_ms": t4 - t3, "preview_ms": t5 - t4,
        })
        .to_string();
        Ok(Conversion {
            dxf,
            report,
            timings,
            preview_positions: preview.positions,
            preview_indices: preview.indices,
            preview_edges: preview.edges,
            preview_colors: preview.colors,
            preview_lines: preview.lines,
            preview_origin: preview.origin.to_vec(),
            preview_groups: preview.groups,
            preview_points: preview.points,
            preview_available: preview.available,
        })
    }
}

/// A finished conversion. Preview buffers are for display only and are *not* the output:
/// positions are float32 and re-centered on `origin`. Each `take_*` moves its buffer out
/// (one copy into JS instead of a clone plus a copy); call each once.
#[wasm_bindgen]
pub struct Conversion {
    dxf: Vec<u8>,
    report: String,
    timings: String,
    preview_positions: Vec<f32>,
    preview_indices: Vec<u32>,
    preview_edges: Vec<u32>,
    preview_colors: Vec<u8>,
    preview_lines: Vec<f32>,
    preview_origin: Vec<f64>,
    preview_groups: Vec<u32>,
    preview_points: Vec<f32>,
    preview_available: bool,
}

#[wasm_bindgen]
impl Conversion {
    /// Point elements (`p`), xyz float32 each.
    pub fn take_points(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.preview_points)
    }
    /// False when the model's extent is too large to display in float32 (the DXF is
    /// unaffected); the preview buffers are then empty.
    pub fn preview_available(&self) -> bool {
        self.preview_available
    }
    /// Per mesh entity: `[layer, index_start, index_count, edge_start, edge_count]`, so
    /// the viewer can show or hide layers without rebuilding buffers.
    pub fn take_groups(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.preview_groups)
    }
    pub fn take_dxf(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.dxf)
    }
    /// Report JSON (see `obj2cad_core::report`).
    pub fn report(&self) -> String {
        self.report.clone()
    }
    /// Per-stage milliseconds, as JSON.
    pub fn timings(&self) -> String {
        self.timings.clone()
    }
    /// Triangle-fan preview: xyz float32 per vertex.
    pub fn take_positions(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.preview_positions)
    }
    pub fn take_indices(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.preview_indices)
    }
    /// True polygon edges (index pairs), not the display triangles. Edges shared by two
    /// faces appear twice; drawing them twice is cheaper than deduplicating.
    pub fn take_edges(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.preview_edges)
    }
    /// RGB (0–255) per preview vertex: entity color, or a stable color per layer.
    pub fn take_colors(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.preview_colors)
    }
    /// Line segments (pairs of xyz) for OBJ `l` elements.
    pub fn take_lines(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.preview_lines)
    }
    pub fn origin(&self) -> Vec<f64> {
        self.preview_origin.clone()
    }
}

struct Preview {
    positions: Vec<f32>,
    indices: Vec<u32>,
    edges: Vec<u32>,
    colors: Vec<u8>,
    lines: Vec<f32>,
    origin: [f64; 3],
    groups: Vec<u32>,
    points: Vec<f32>,
    available: bool,
}

impl Preview {
    fn build(model: &obj2cad_core::CadModel) -> Self {
        let origin = model
            .bounds()
            .map(|(lo, hi)| [0, 1, 2].map(|a| (lo[a] + hi[a]) / 2.0))
            .unwrap_or([0.0; 3]);
        let rel = |v: u32| {
            let p = model.position(v);
            [0, 1, 2].map(|a| (p[a] - origin[a]) as f32)
        };
        let nv: usize = model.meshes.iter().map(|m| m.vertices.len()).sum();
        let refs: usize = model.meshes.iter().map(|m| m.face_indices.len()).sum();
        let mut out = Preview {
            positions: Vec::with_capacity(nv * 3),
            indices: Vec::with_capacity(refs * 3),
            edges: Vec::with_capacity(refs * 2),
            colors: Vec::with_capacity(nv * 3),
            lines: Vec::new(),
            origin,
            groups: Vec::with_capacity(model.meshes.len() * 5),
            points: Vec::with_capacity(model.points.len() * 3),
            available: true,
        };
        // float32 (and three.js's squared-length math) can't display extents beyond this;
        // show an honest "no preview" rather than a broken one.
        let displayable = model.bounds().is_none_or(|(lo, hi)| {
            (0..3).all(|a| (hi[a] - lo[a]).is_finite() && hi[a] - lo[a] < 1e12)
        });
        if !displayable {
            out.available = false;
            return out;
        }
        for m in &model.meshes {
            let base = (out.positions.len() / 3) as u32;
            let (i0, e0) = (out.indices.len() as u32, out.edges.len() as u32);
            let rgb = m.color.unwrap_or_else(|| layer_color(m.layer));
            for &v in &m.vertices {
                out.positions.extend_from_slice(&rel(v));
                out.colors.extend_from_slice(&rgb);
            }
            for f in m.faces() {
                for k in 1..f.len() - 1 {
                    out.indices
                        .extend_from_slice(&[base + f[0], base + f[k], base + f[k + 1]]);
                }
                for k in 0..f.len() {
                    out.edges
                        .extend_from_slice(&[base + f[k], base + f[(k + 1) % f.len()]]);
                }
            }
            let (i1, e1) = (out.indices.len() as u32, out.edges.len() as u32);
            out.groups
                .extend_from_slice(&[m.layer, i0, i1 - i0, e0, e1 - e0]);
        }
        for p in &model.points {
            out.points.extend_from_slice(&rel(p.vertex));
        }
        for l in &model.polylines {
            for w in l.vertices.windows(2) {
                out.lines.extend_from_slice(&rel(w[0]));
                out.lines.extend_from_slice(&rel(w[1]));
            }
        }
        out
    }
}

fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys_now()
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
    fn js_sys_now() -> f64;
}

/// Calm, distinguishable preview colors for layers without a material color.
fn layer_color(layer: u32) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 8] = [
        [176, 184, 196],
        [124, 158, 201],
        [201, 150, 110],
        [128, 180, 150],
        [190, 140, 180],
        [210, 190, 120],
        [120, 170, 180],
        [180, 130, 130],
    ];
    PALETTE[layer as usize % PALETTE.len()]
}
