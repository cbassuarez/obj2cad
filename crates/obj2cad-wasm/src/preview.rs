//! Display buffers for the web preview. Display only: float32 positions re-centered on the
//! model's bounds. The exported file is written separately from the exact doubles.

use obj2cad_core::CadModel;

pub struct Preview {
    pub positions: Vec<f32>,
    pub colors: Vec<u8>,
    pub indices: Vec<u32>,
    /// True polygon edges (index pairs), not the display triangles.
    pub edges: Vec<u32>,
    /// Per layer (all its mesh entities, whatever their colors): `[layer, index_start,
    /// index_count, edge_start, edge_count]`.
    pub groups: Vec<u32>,
    /// Line segments (pairs of xyz) and their colors (rgb per vertex), grouped by layer.
    pub lines: Vec<f32>,
    pub line_colors: Vec<u8>,
    /// Per layer with lines: `[layer, vertex_start, vertex_count]`.
    pub line_groups: Vec<u32>,
    /// Points (xyz) and their colors, grouped by layer. At most [`MAX_SHOWN_POINTS`]:
    /// larger clouds are shown evenly thinned (every `point_stride`-th point of a layer).
    pub points: Vec<f32>,
    pub point_colors: Vec<u8>,
    /// Per layer with points: `[layer, point_start, point_count]`.
    pub point_groups: Vec<u32>,
    pub point_stride: u32,
    /// Per layer shown: `[layer, min x, y, z, max x, y, z]`, in display coordinates, so
    /// the viewer never scans the buffers for extents.
    pub layer_bounds: Vec<f32>,
    pub origin: [f64; 3],
    /// False when the model is too large to display in float32; buffers are then empty.
    pub available: bool,
}

/// Extents beyond this can't be displayed in float32 (and three.js's squared-length math).
const MAX_EXTENT: f64 = 1e12;

/// Points drawn at most. A browser draws a few million easily; beyond that a cloud is
/// shown thinned, evenly (the file always has every point).
pub const MAX_SHOWN_POINTS: usize = 4_000_000;

/// Per-layer extents, grown as positions are added.
#[derive(Default)]
struct Bounds(std::collections::BTreeMap<u32, [f32; 6]>);

impl Bounds {
    fn grow(&mut self, layer: u32, p: [f32; 3]) {
        let b = self.0.entry(layer).or_insert([
            f32::INFINITY,
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ]);
        for a in 0..3 {
            b[a] = b[a].min(p[a]);
            b[a + 3] = b[a + 3].max(p[a]);
        }
    }

    fn flat(self) -> Vec<f32> {
        self.0
            .into_iter()
            .flat_map(|(layer, b)| std::iter::once(layer as f32).chain(b))
            .collect()
    }
}

impl Preview {
    pub fn build(model: &CadModel) -> Self {
        Self::build_within(model, MAX_SHOWN_POINTS)
    }

    /// [`Self::build`], showing at most `max_points` points.
    fn build_within(model: &CadModel, max_points: usize) -> Self {
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
            point_stride: 1,
            layer_bounds: Vec::new(),
            origin,
            available: true,
        };
        let mut bounds = Bounds::default();
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
        // Colors go out in linear light, as the renderer takes vertex colors.
        let linear: [u8; 256] =
            std::array::from_fn(|i| (obj2cad_core::color::TO_LINEAR[i] * 255.0).round() as u8);
        let color = |c: Option<[u8; 3]>, layer: u32| {
            c.unwrap_or(model.layers[layer as usize].color)
                .map(|x| linear[x as usize])
        };

        let mut tri = Triangulator::default();
        // Layer by layer, so each layer is one group (one draw) however many colors or
        // entities it has: a textured model can have hundreds.
        let mut by_layer: Vec<usize> = (0..model.meshes.len()).collect();
        by_layer.sort_by_key(|&i| model.meshes[i].layer);
        for m in by_layer.into_iter().map(|i| &model.meshes[i]) {
            let base = (out.positions.len() / 3) as u32;
            let (i0, e0) = (out.indices.len() as u32, out.edges.len() as u32);
            let rgb = color(m.color, m.layer);
            let local: Vec<[f64; 3]> = m.vertices.iter().map(|&v| rel(v)).collect();
            for p in &local {
                let f = p.map(|c| c as f32);
                bounds.grow(m.layer, f);
                out.positions.extend(f);
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
            let n = out.groups.len();
            if n >= 5 && out.groups[n - 5] == m.layer {
                // The same layer as the group before: its ranges continue it.
                out.groups[n - 3] += i1 - i0;
                out.groups[n - 1] += e1 - e0;
            } else {
                out.groups
                    .extend_from_slice(&[m.layer, i0, i1 - i0, e0, e1 - e0]);
            }
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
                    let f = rel(*v).map(|c| c as f32);
                    bounds.grow(l.layer, f);
                    out.lines.extend(f);
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
        // Free-form curves, sampled for display (the file keeps them exact).
        let mut by_layer: Vec<usize> = (0..model.splines.len()).collect();
        by_layer.sort_by_key(|&i| model.splines[i].layer);
        for i in by_layer {
            let c = &model.splines[i];
            let start = (out.lines.len() / 3) as u32;
            let rgb = color(c.color, c.layer);
            let cps: Vec<[f64; 3]> = c.control.iter().map(|&v| rel(v)).collect();
            let pts = sample_spline(c.degree as usize, &c.knots, &cps, c.weights.as_deref());
            for w in pts.windows(2) {
                for p in w {
                    let f = p.map(|x| x as f32);
                    bounds.grow(c.layer, f);
                    out.lines.extend(f);
                    out.line_colors.extend_from_slice(&rgb);
                }
            }
            push_group(
                &mut out.line_groups,
                c.layer,
                start,
                (out.lines.len() / 3) as u32 - start,
            );
        }
        let mut by_layer: Vec<usize> = (0..model.points.len()).collect();
        by_layer.sort_by_key(|&i| model.points[i].layer);
        let stride = model.points.len().div_ceil(max_points.max(1)).max(1);
        out.point_stride = stride as u32;
        let (mut layer, mut k) = (u32::MAX, 0usize);
        for i in by_layer {
            let p = &model.points[i];
            // Every `stride`-th point of each layer (a layer's extent still counts them all).
            if p.layer != layer {
                (layer, k) = (p.layer, 0);
            }
            let f = rel(p.vertex).map(|c| c as f32);
            bounds.grow(p.layer, f);
            k += 1;
            if (k - 1) % stride != 0 {
                continue;
            }
            let start = (out.points.len() / 3) as u32;
            out.points.extend(f);
            out.point_colors.extend_from_slice(&color(p.color, p.layer));
            push_group(&mut out.point_groups, p.layer, start, 1);
        }
        // Recognized curved surfaces: their exact edges (closed surfaces, a few circles),
        // drawn as lines over the mesh on their own layer.
        let mut by_layer: Vec<usize> = (0..model.surfaces.len()).collect();
        by_layer.sort_by_key(|&i| model.surfaces[i].layer);
        for i in by_layer {
            let s = &model.surfaces[i];
            let start = (out.lines.len() / 3) as u32;
            let rgb = color(s.color, s.layer);
            for poly in outline(&s.body) {
                for w in poly.windows(2) {
                    for p in w {
                        let f = [0, 1, 2].map(|a| (p[a] - origin[a]) as f32);
                        bounds.grow(s.layer, f);
                        out.lines.extend(f);
                        out.line_colors.extend_from_slice(&rgb);
                    }
                }
            }
            push_group(
                &mut out.line_groups,
                s.layer,
                start,
                (out.lines.len() / 3) as u32 - start,
            );
        }
        out.layer_bounds = bounds.flat();
        out
    }
}

/// Points along a (rational) B-spline over its knot domain, 16 per knot span.
fn sample_spline(
    deg: usize,
    knots: &[f64],
    cps: &[[f64; 3]],
    weights: Option<&[f64]>,
) -> Vec<[f64; 3]> {
    let n = cps.len();
    if n <= deg || knots.len() != n + deg + 1 {
        return cps.to_vec();
    }
    let w = |i: usize| weights.map_or(1.0, |w| w[i]);
    // de Boor in homogeneous coordinates.
    let eval = |t: f64| -> [f64; 3] {
        let mut k = deg;
        while k + 1 < n && knots[k + 1] <= t {
            k += 1;
        }
        let mut d: Vec<[f64; 4]> = (0..=deg)
            .map(|j| {
                let i = k + j - deg;
                let p = cps[i];
                [p[0] * w(i), p[1] * w(i), p[2] * w(i), w(i)]
            })
            .collect();
        for r in 1..=deg {
            for j in (r..=deg).rev() {
                let i = k + j - deg;
                let den = knots[i + deg + 1 - r] - knots[i];
                let a = if den == 0.0 {
                    0.0
                } else {
                    (t - knots[i]) / den
                };
                let prev = d[j - 1];
                for (x, p) in d[j].iter_mut().zip(prev) {
                    *x = (1.0 - a) * p + a * *x;
                }
            }
        }
        let h = d[deg];
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    };
    let (lo, hi) = (knots[deg], knots[n]);
    let spans = knots[deg..=n]
        .windows(2)
        .filter(|w| w[1] > w[0])
        .count()
        .max(1);
    let steps = 16 * spans;
    (0..=steps)
        .map(|s| eval(lo + (hi - lo) * s as f64 / steps as f64))
        .collect()
}

/// Polylines that show an ACIS body: its edges, sampled; for closed faces (no edges), a
/// few circles on the surface.
fn outline(body: &obj2cad_acis::Body) -> Vec<Vec<[f64; 3]>> {
    use obj2cad_acis::{Curve, Surface};
    let mut out = Vec::new();
    let circle = |center: [f64; 3], normal: [f64; 3], radius: f64| {
        let u = obj2cad_acis::perpendicular(normal);
        let c = Curve::Circle {
            center,
            normal,
            u_dir: u,
            radius,
        };
        (0..=64)
            .map(|k| c.point(std::f64::consts::TAU * f64::from(k) / 64.0))
            .collect::<Vec<_>>()
    };
    for e in &body.edges {
        let n = if matches!(e.curve, Curve::Line { .. }) {
            1
        } else {
            48
        };
        out.push(
            (0..=n)
                .map(|k| {
                    e.curve
                        .point(e.t0 + (e.t1 - e.t0) * f64::from(k) / f64::from(n))
                })
                .collect(),
        );
    }
    for f in body.faces.iter().filter(|f| f.loops.is_empty()) {
        match f.surface {
            Surface::Sphere {
                center,
                radius,
                u_dir,
                pole,
            } => {
                out.push(circle(center, pole, radius));
                out.push(circle(center, u_dir, radius));
                out.push(circle(center, obj2cad_acis::cross(pole, u_dir), radius));
            }
            Surface::Torus {
                center,
                axis,
                major,
                minor,
                u_dir,
            } => {
                out.push(circle(center, axis, major + minor));
                out.push(circle(center, axis, major - minor));
                let up = obj2cad_acis::scale(axis, minor);
                out.push(circle(obj2cad_acis::add(center, up), axis, major));
                out.push(circle(obj2cad_acis::sub(center, up), axis, major));
                let tube = obj2cad_acis::add(center, obj2cad_acis::scale(u_dir, major));
                out.push(circle(tube, obj2cad_acis::cross(axis, u_dir), minor));
            }
            _ => {}
        }
    }
    out
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
    fn each_layer_is_one_group_whatever_its_colors() {
        // Layer a: a red face and, after a face on layer b, a blue one.
        let p = preview(
            "v 0 0 0 1 0 0\nv 1 0 0 1 0 0\nv 1 1 0 1 0 0\nv 0 0 1 0 0 1\nv 1 0 1 0 0 1\nv 1 1 1 0 0 1\n\
             o a\nf 1 2 3\no b\nf 1 2 3\no a\nf 4 5 6\n",
        );
        let layers: Vec<u32> = p.groups.chunks(5).map(|g| g[0]).collect();
        assert_eq!(layers, vec![1, 2]);
        assert_eq!(p.groups[2], 6, "both of layer a's triangles in its group");
    }

    #[test]
    fn layer_colors_come_from_the_engine() {
        let p = preview("o a\nv 0 0 0\nv 1 0 0\nv 1 1 0\nf 1 2 3\n");
        // In linear light, as the renderer takes them.
        let lin = obj2cad_core::convert::layer_color(1)
            .map(|c| (obj2cad_core::color::TO_LINEAR[c as usize] * 255.0).round() as u8);
        assert_eq!(&p.colors[..3], &lin);
    }

    #[test]
    fn big_clouds_are_shown_thinned_with_their_full_extent() {
        let src: String = (0..10).map(|i| format!("{i} 0 0\n")).collect();
        let doc = obj2cad_core::xyz::parse(src.as_bytes(), "scan.xyz").unwrap();
        let model = obj2cad_core::convert(&doc, None, obj2cad_core::Options::default());
        let p = Preview::build_within(&model, 4);
        assert_eq!((p.point_stride, p.points.len() / 3), (3, 4)); // points 0, 3, 6, 9
                                                                  // One layer, and its extent covers all ten points (x from -4.5 to 4.5 around the middle).
        assert_eq!(p.layer_bounds, vec![1.0, -4.5, 0.0, 0.0, 4.5, 0.0, 0.0]);
    }

    #[test]
    fn rational_splines_sample_exactly_onto_their_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let pts = sample_spline(
            2,
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[[10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            Some(&[1.0, w, 1.0]),
        );
        assert_eq!(pts.len(), 17);
        assert!(pts
            .iter()
            .all(|p| ((p[0] * p[0] + p[1] * p[1]).sqrt() - 10.0).abs() < 1e-9));
    }

    #[test]
    fn huge_models_have_no_preview() {
        let p = preview("v -1e300 0 0\nv 1e300 0 0\nv 0 1 0\nf 1 2 3\n");
        assert!(!p.available && p.positions.is_empty());
    }
}
