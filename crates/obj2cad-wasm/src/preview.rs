//! Display buffers for the web preview. Display only: float32 positions re-centered on the
//! model's bounds. The exported file is written separately from the exact doubles.

use obj2cad_core::CadModel;

pub struct Preview {
    pub positions: Vec<f32>,
    pub colors: Vec<u8>,
    pub indices: Vec<u32>,
    /// True polygon edges (index pairs), not the display triangles.
    pub edges: Vec<u32>,
    /// Per mesh entity: `[layer, index_start, index_count, edge_start, edge_count]`.
    pub groups: Vec<u32>,
    /// Line segments (pairs of xyz) and their colors (rgb per vertex), grouped by layer.
    pub lines: Vec<f32>,
    pub line_colors: Vec<u8>,
    /// Per layer with lines: `[layer, vertex_start, vertex_count]`.
    pub line_groups: Vec<u32>,
    /// Points (xyz) and their colors, grouped by layer.
    pub points: Vec<f32>,
    pub point_colors: Vec<u8>,
    /// Per layer with points: `[layer, point_start, point_count]`.
    pub point_groups: Vec<u32>,
    pub origin: [f64; 3],
    /// False when the model is too large to display in float32; buffers are then empty.
    pub available: bool,
}

/// Extents beyond this can't be displayed in float32 (and three.js's squared-length math).
const MAX_EXTENT: f64 = 1e12;

impl Preview {
    pub fn build(model: &CadModel) -> Self {
        let origin = model
            .bounds()
            .map(|(lo, hi)| [0, 1, 2].map(|a| lo[a] / 2.0 + hi[a] / 2.0))
            .unwrap_or([0.0; 3]);
        let nv: usize = model.meshes.iter().map(|m| m.vertices.len()).sum();
        let refs: usize = model.meshes.iter().map(|m| m.face_indices.len()).sum();
        let mut out = Preview {
            positions: Vec::with_capacity(nv * 3),
            colors: Vec::with_capacity(nv * 3),
            indices: Vec::with_capacity(refs * 3),
            edges: Vec::with_capacity(refs * 2),
            groups: Vec::with_capacity(model.meshes.len() * 5),
            lines: Vec::new(),
            line_colors: Vec::new(),
            line_groups: Vec::new(),
            points: Vec::with_capacity(model.points.len() * 3),
            point_colors: Vec::with_capacity(model.points.len() * 3),
            point_groups: Vec::new(),
            origin,
            available: true,
        };
        let displayable = model.bounds().is_none_or(|(lo, hi)| {
            (0..3).all(|a| (hi[a] - lo[a]).is_finite() && hi[a] - lo[a] < MAX_EXTENT)
        });
        if !displayable {
            out.available = false;
            return out;
        }
        // Relative positions in f64 for triangulation; f32 only for display.
        let rel = |v: u32| {
            let p = model.position(v);
            [0, 1, 2].map(|a| p[a] - origin[a])
        };
        let color =
            |c: Option<[u8; 3]>, layer: u32| c.unwrap_or(model.layers[layer as usize].color);

        let mut tri = Triangulator::default();
        for m in &model.meshes {
            let base = (out.positions.len() / 3) as u32;
            let (i0, e0) = (out.indices.len() as u32, out.edges.len() as u32);
            let rgb = color(m.color, m.layer);
            let local: Vec<[f64; 3]> = m.vertices.iter().map(|&v| rel(v)).collect();
            for p in &local {
                out.positions.extend(p.map(|c| c as f32));
                out.colors.extend_from_slice(&rgb);
            }
            for f in m.faces() {
                tri.triangulate(f, &local, base, &mut out.indices);
                for k in 0..f.len() {
                    out.edges
                        .extend_from_slice(&[base + f[k], base + f[(k + 1) % f.len()]]);
                }
            }
            let (i1, e1) = (out.indices.len() as u32, out.edges.len() as u32);
            out.groups
                .extend_from_slice(&[m.layer, i0, i1 - i0, e0, e1 - e0]);
        }

        // Lines and points, grouped by layer so the viewer can hide layers.
        let mut by_layer: Vec<usize> = (0..model.polylines.len()).collect();
        by_layer.sort_by_key(|&i| model.polylines[i].layer);
        for i in by_layer {
            let l = &model.polylines[i];
            let start = (out.lines.len() / 3) as u32;
            let rgb = color(l.color, l.layer);
            for w in l.vertices.windows(2) {
                for v in w {
                    out.lines.extend(rel(*v).map(|c| c as f32));
                    out.line_colors.extend_from_slice(&rgb);
                }
            }
            push_group(
                &mut out.line_groups,
                l.layer,
                start,
                (out.lines.len() / 3) as u32 - start,
            );
        }
        let mut by_layer: Vec<usize> = (0..model.points.len()).collect();
        by_layer.sort_by_key(|&i| model.points[i].layer);
        for i in by_layer {
            let p = &model.points[i];
            let start = (out.points.len() / 3) as u32;
            out.points.extend(rel(p.vertex).map(|c| c as f32));
            out.point_colors.extend_from_slice(&color(p.color, p.layer));
            push_group(&mut out.point_groups, p.layer, start, 1);
        }
        out
    }
}

/// Append `count` items at `start` to `layer`'s group, merging with the previous group
/// when it is the same layer (inputs are sorted by layer).
fn push_group(groups: &mut Vec<u32>, layer: u32, start: u32, count: u32) {
    if count == 0 {
        return;
    }
    let n = groups.len();
    if n >= 3 && groups[n - 3] == layer && groups[n - 2] + groups[n - 1] == start {
        groups[n - 1] += count;
    } else {
        groups.extend_from_slice(&[layer, start, count]);
    }
}

/// Triangles for display: a fan for convex faces (almost all of them), ear clipping for
/// concave ones, so an L-shaped or star-shaped n-gon looks like itself.
#[derive(Default)]
struct Triangulator {
    earcut: earcut::Earcut<f64>,
    ring: Vec<[f64; 3]>,
    flat: Vec<[f64; 2]>,
    tris: Vec<u32>,
}

impl Triangulator {
    fn triangulate(&mut self, face: &[u32], local: &[[f64; 3]], base: u32, out: &mut Vec<u32>) {
        let n = face.len();
        let fan = |out: &mut Vec<u32>| {
            for k in 1..n - 1 {
                out.extend_from_slice(&[base + face[0], base + face[k], base + face[k + 1]]);
            }
        };
        if n == 3 {
            return fan(out);
        }
        self.ring.clear();
        self.ring.extend(face.iter().map(|&i| local[i as usize]));
        let normal = newell(&self.ring);
        if is_convex(&self.ring, normal) {
            return fan(out);
        }
        if earcut::utils3d::project3d_to_2d(&self.ring, n, &mut self.flat) {
            self.earcut
                .earcut(self.flat.iter().copied(), &[] as &[u32], &mut self.tris);
        } else {
            self.tris.clear();
        }
        // Degenerate (collinear or self-overlapping) faces: fall back to the fan so the
        // face is still visible.
        if self.tris.len() != 3 * (n - 2) {
            return fan(out);
        }
        out.extend(self.tris.iter().map(|&k| base + face[k as usize]));
    }
}

fn newell(ring: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for (i, a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// Every corner turns the same way around `normal` (collinear corners allowed).
fn is_convex(ring: &[[f64; 3]], normal: [f64; 3]) -> bool {
    let n = ring.len();
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let scale = normal.iter().map(|c| c.abs()).fold(0.0, f64::max);
    if scale == 0.0 {
        return true; // degenerate: nothing better than a fan
    }
    (0..n).all(|i| {
        let (p, c, q) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        let (e1, e2) = (sub(c, p), sub(q, c));
        let cross = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] >= -1e-12 * scale * scale
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use obj2cad_core::{convert, parse, Options};

    fn preview(src: &str) -> Preview {
        let doc = parse(src.as_bytes()).unwrap();
        Preview::build(&convert(&doc, None, Options::default()))
    }

    /// Area of the display triangles, from the f32 positions.
    fn area(p: &Preview) -> f64 {
        let v = |i: u32| [0, 1, 2].map(|a| f64::from(p.positions[i as usize * 3 + a]));
        p.indices
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
                let (u, w) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
                (u[0] * w[1] - u[1] * w[0]).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn concave_faces_keep_their_shape() {
        // A U shape (area 5): a fan from the first corner overlaps itself (area 7).
        let u = "v 0 0 0\nv 3 0 0\nv 3 2 0\nv 2 2 0\nv 2 1 0\nv 1 1 0\nv 1 2 0\nv 0 2 0\nf 1 2 3 4 5 6 7 8\n";
        let p = preview(u);
        assert_eq!(p.indices.len(), 3 * 6);
        assert!((area(&p) - 5.0).abs() < 1e-9, "area {}", area(&p));
    }

    #[test]
    fn convex_faces_use_a_fan() {
        let p = preview("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n");
        assert_eq!(p.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(p.edges.len(), 8);
    }

    #[test]
    fn degenerate_faces_still_show() {
        let p = preview("v 0 0 0\nv 1 0 0\nv 2 0 0\nv 3 0 0\nf 1 2 3 4\n");
        assert_eq!(p.indices.len(), 6);
    }

    #[test]
    fn lines_and_points_are_grouped_by_layer() {
        let p = preview("v 0 0 0\nv 1 0 0\nv 1 1 0\no a\nl 1 2 3\np 1\no b\nl 1 3\np 2 3\n");
        assert_eq!(p.line_groups, vec![1, 0, 4, 2, 4, 2]);
        assert_eq!(p.point_groups, vec![1, 0, 1, 2, 1, 2]);
        assert_eq!(p.line_colors.len(), p.lines.len());
    }

    #[test]
    fn layer_colors_come_from_the_engine() {
        let p = preview("o a\nv 0 0 0\nv 1 0 0\nv 1 1 0\nf 1 2 3\n");
        assert_eq!(&p.colors[..3], &obj2cad_core::convert::layer_color(1));
    }

    #[test]
    fn huge_models_have_no_preview() {
        let p = preview("v -1e300 0 0\nv 1e300 0 0\nv 0 1 0\nf 1 2 3\n");
        assert!(!p.available && p.positions.is_empty());
    }
}
