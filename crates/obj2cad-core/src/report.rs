//! The parity report written next to every conversion.

use crate::convert::{CadModel, Omissions, Options, UpAxis};
use crate::diag::Diagnostic;
use crate::obj::SourceCounts;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub engine_version: &'static str,
    pub input: Input,
    pub output: Output,
    pub options: Options,
    /// `obj2cad-parity-v1` hash of the geometry as written (see `hash.rs`).
    pub parity_hash: String,
    /// The model was stood upright (an exact axis swap).
    pub rotated: bool,
    /// The file had only vertices; they were written as points.
    pub point_cloud: bool,
    /// Geometry in the source that is not in the output.
    pub omissions: Omissions,
    pub layers: Vec<LayerSummary>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub struct LayerSummary {
    /// Name in the drawing (DXF-safe, unique).
    pub name: String,
    /// The OBJ name it came from, verbatim ("" for layer 0 and the default layer).
    pub source: String,
    /// Layer color in the drawing, `#rrggbb`.
    pub color: String,
    /// Material colors of entities on this layer (they override the layer color).
    pub entity_colors: Vec<String>,
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
    /// `None` until computed (the web app hashes the file when the report is saved).
    pub sha256: Option<String>,
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

/// The written file as the report describes it.
pub struct Written<'a> {
    pub format: &'a str,
    pub bytes: u64,
    pub sha256: Option<String>,
}

pub fn hex_color([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn build(model: &CadModel, source: &Source, parity: &str, written: Written) -> Report {
    let d = model.doc;
    // Per-layer tallies in one pass over the entities.
    let n = model.layers.len();
    let (mut faces, mut polylines, mut points) = (vec![0u64; n], vec![0u64; n], vec![0u64; n]);
    let mut colors: Vec<BTreeSet<[u8; 3]>> = vec![BTreeSet::new(); n];
    for m in &model.meshes {
        faces[m.layer as usize] += m.face_count() as u64;
        colors[m.layer as usize].extend(m.color);
    }
    for l in &model.polylines {
        polylines[l.layer as usize] += 1;
        colors[l.layer as usize].extend(l.color);
    }
    for p in &model.points {
        points[p.layer as usize] += 1;
        colors[p.layer as usize].extend(p.color);
    }
    Report {
        schema: "obj2cad-report-v2",
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
            format: written.format.to_owned(),
            bytes: written.bytes,
            sha256: written.sha256,
            mesh_entities: model.meshes.len() as u64,
            faces: faces.iter().sum(),
            polylines: model.polylines.len() as u64,
            points: model.points.len() as u64,
            vertices_written: model.meshes.iter().map(|m| m.vertices.len() as u64).sum::<u64>()
                + model.polylines.iter().map(|l| l.vertices.len() as u64).sum::<u64>()
                + model.points.len() as u64,
            unreferenced_vertices_skipped: model.omissions.loose_points,
            bounds: model.bounds(),
        },
        options: model.options.clone(),
        parity_hash: parity.to_owned(),
        rotated: model.options.up_axis != UpAxis::AsIs,
        point_cloud: model.point_cloud,
        omissions: model.omissions.clone(),
        layers: model
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| LayerSummary {
                name: l.name.clone(),
                source: l.source.clone(),
                color: hex_color(l.color),
                entity_colors: colors[i].iter().map(|&c| hex_color(c)).collect(),
                faces: faces[i],
                polylines: polylines[i],
                points: points[i],
            })
            .collect(),
        diagnostics: model.diagnostics.clone(),
    }
}
