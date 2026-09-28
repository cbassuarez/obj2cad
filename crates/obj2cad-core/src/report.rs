//! The parity report written next to every conversion.

use crate::convert::{CadModel, Options};
use crate::diag::Diagnostic;
use crate::hash::sha256_hex;
use crate::obj::SourceCounts;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub engine_version: &'static str,
    pub input: Input,
    pub output: Output,
    pub options: Options,
    /// `obj2cad-parity-v1` hash of the geometry as written (see `hash.rs`).
    pub parity_hash: String,
    pub layers: Vec<LayerSummary>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub struct LayerSummary {
    /// Name in the drawing (DXF-safe, unique).
    pub name: String,
    /// The OBJ object/group it came from, verbatim ("" for the default layer 0).
    pub source: String,
    pub faces: u64,
    pub polylines: u64,
    pub points: u64,
}

#[derive(Debug, Serialize)]
pub struct Input {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub vertices: u64,
    pub faces: u64,
    pub lines: u64,
    pub point_elements: u64,
    pub objects: u64,
    pub groups: u64,
    pub materials: u64,
    pub counts: SourceCounts,
    pub header_comments: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Output {
    pub format: String,
    pub bytes: u64,
    pub sha256: String,
    pub mesh_entities: u64,
    pub faces: u64,
    pub polylines: u64,
    pub points: u64,
    pub vertices_written: u64,
    pub unreferenced_vertices_skipped: u64,
    pub bounds: Option<([f64; 3], [f64; 3])>,
}

/// The source file as the report describes it. Hashes are computed by the caller
/// (hashing a large source twice is measurable in the browser).
pub struct Source<'a> {
    pub name: &'a str,
    pub len: u64,
    pub sha256: &'a str,
}

/// `output_sha256: None` hashes the output here; the web app passes the browser's
/// hardware-accelerated digest instead.
pub fn build(
    model: &CadModel,
    source: &Source,
    parity: &str,
    format: &str,
    output_bytes: &[u8],
    output_sha256: Option<&str>,
) -> Report {
    let d = model.doc;
    Report {
        schema: "obj2cad-report-v1",
        engine_version: crate::VERSION,
        input: Input {
            name: source.name.to_owned(),
            bytes: source.len,
            sha256: source.sha256.to_owned(),
            vertices: d.positions.len() as u64,
            faces: d.faces.len() as u64,
            lines: d.lines.len() as u64,
            point_elements: d.points.len() as u64,
            objects: d.objects.len() as u64,
            groups: d.groups.len() as u64,
            materials: d.materials.len() as u64,
            counts: d.counts.clone(),
            header_comments: d.header_comments.clone(),
        },
        output: Output {
            format: format.to_owned(),
            bytes: output_bytes.len() as u64,
            sha256: output_sha256.map_or_else(|| sha256_hex(output_bytes), str::to_owned),
            mesh_entities: model.meshes.len() as u64,
            faces: model.meshes.iter().map(|m| m.face_count() as u64).sum(),
            polylines: model.polylines.len() as u64,
            points: model.points.len() as u64,
            vertices_written: model
                .meshes
                .iter()
                .map(|m| m.vertices.len() as u64)
                .sum::<u64>()
                + model
                    .polylines
                    .iter()
                    .map(|l| l.vertices.len() as u64)
                    .sum::<u64>()
                + model.points.len() as u64,
            unreferenced_vertices_skipped: model.unreferenced_vertices,
            bounds: model.bounds(),
        },
        options: model.options.clone(),
        parity_hash: parity.to_owned(),
        layers: model
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| LayerSummary {
                name: l.name.clone(),
                source: l.source.clone(),
                faces: model
                    .meshes
                    .iter()
                    .filter(|m| m.layer as usize == i)
                    .map(|m| m.face_count() as u64)
                    .sum(),
                polylines: model
                    .polylines
                    .iter()
                    .filter(|p| p.layer as usize == i)
                    .count() as u64,
                points: model
                    .points
                    .iter()
                    .filter(|p| p.layer as usize == i)
                    .count() as u64,
            })
            .collect(),
        diagnostics: model.diagnostics.clone(),
    }
}
