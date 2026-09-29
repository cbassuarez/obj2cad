//! The parity report written next to every conversion.

use crate::bundle::FileEntry;
use crate::convert::{CadModel, Omissions, Options, Region, UpAxis};
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
    /// Faces colored from texture images (approximate colors; shapes are exact).
    pub texture_colored_faces: u64,
    /// Faces colored with the average of their vertices' colors (approximate colors).
    pub vertex_colored_faces: u64,
    /// Every file in the bundle and what it was used for (empty for a single file).
    pub files: Vec<FileEntry>,
    /// Curved surfaces written next to the mesh, in the order of their entities.
    pub curves: Vec<Region>,
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
    /// Colors of entities on this layer (they override the layer color), at most
    /// [`MAX_LISTED_COLORS`]; `more_colors` when there are more.
    pub entity_colors: Vec<String>,
    pub more_colors: bool,
    pub faces: u64,
    pub polylines: u64,
    pub points: u64,
    /// Curved surfaces (ACIS bodies).
    pub surfaces: u64,
    /// The file of a bundle this layer's geometry came from (absent for a single file,
    /// or when the layer holds geometry of several files).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
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
    /// Free-form curves, as B-splines.
    pub splines: u64,
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
    pub files: &'a [FileEntry],
}

/// How many entity colors a layer summary lists.
pub const MAX_LISTED_COLORS: usize = 16;

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
    // Colored point clouds can have millions of colors; only a few are listed.
    let mut add = |layer: u32, c: Option<[u8; 3]>| {
        let set = &mut colors[layer as usize];
        if let Some(c) = c {
            if set.len() <= MAX_LISTED_COLORS {
                set.insert(c);
            }
        }
    };
    for m in &model.meshes {
        faces[m.layer as usize] += m.face_count() as u64;
        add(m.layer, m.color);
    }
    for l in &model.polylines {
        polylines[l.layer as usize] += 1;
        add(l.layer, l.color);
    }
    for c in &model.splines {
        polylines[c.layer as usize] += 1;
        add(c.layer, c.color);
    }
    for p in &model.points {
        points[p.layer as usize] += 1;
        add(p.layer, p.color);
    }
    let mut surfaces = vec![0u64; n];
    for s in &model.surfaces {
        surfaces[s.layer as usize] += 1;
        add(s.layer, s.color);
    }
    Report {
        schema: "obj2cad-report-v3",
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
            splines: model.splines.len() as u64,
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
            unreferenced_vertices_skipped: model.omissions.loose_points,
            bounds: model.bounds(),
        },
        options: model.options.clone(),
        parity_hash: parity.to_owned(),
        rotated: model.options.up_axis != UpAxis::AsIs,
        point_cloud: model.point_cloud,
        omissions: model.omissions.clone(),
        texture_colored_faces: model.texture_colored_faces,
        vertex_colored_faces: model.vertex_colored_faces,
        curves: model
            .surfaces
            .iter()
            .filter_map(|s| s.region.clone())
            .collect(),
        files: if source.files.len() > 1 {
            source.files.to_vec()
        } else {
            Vec::new()
        },
        layers: model
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| LayerSummary {
                name: l.name.clone(),
                source: l.source.clone(),
                color: hex_color(l.color),
                entity_colors: colors[i]
                    .iter()
                    .take(MAX_LISTED_COLORS)
                    .map(|&c| hex_color(c))
                    .collect(),
                more_colors: colors[i].len() > MAX_LISTED_COLORS,
                faces: faces[i],
                polylines: polylines[i],
                points: points[i],
                surfaces: surfaces[i],
                file: l.file.clone(),
            })
            .collect(),
        diagnostics: model.diagnostics.clone(),
    }
}
