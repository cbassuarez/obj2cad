//! A small, exact ACIS writer: the boundary representation (B-rep) that AutoCAD stores
//! inside 3DSOLID, BODY and SURFACE entities.
//!
//! [`Body`] is a neutral description (vertices, edges on lines and circles, faces on
//! planes, cones, spheres and tori). [`sab`] writes it as SAB (binary, ACIS 21800 with
//! an ASM header, as DXF R2013+ and DWG R2013+ store it) and [`sat`] as SAT text
//! (ACIS 7.0). Every body is checked by [`Body::validate`] before it is written: each
//! vertex lies on its edges' curves and faces' surfaces, each edge on its faces'
//! surfaces, and each loop closes.
//!
//! Record layouts follow the ACIS 7.0 SAT format as written by AutoCAD and read by
//! ezdxf and acadrust (both used to verify obj2cad's output independently).

pub mod builder;
mod geom;
mod records;
pub mod sab;
pub mod samples;
pub mod sat;

pub use geom::*;

/// A surface a face lies on. All direction vectors are unit length.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    Plane {
        root: V3,
        normal: V3,
        u_dir: V3,
    },
    /// A circular cone or cylinder. `center` is on the axis; the radius there is
    /// `radius`, and it changes by `slope` per unit along `axis` (0 for a cylinder).
    /// `u_dir` (⊥ axis) is where the angular parameter starts.
    Cone {
        center: V3,
        axis: V3,
        u_dir: V3,
        radius: f64,
        slope: f64,
    },
    Sphere {
        center: V3,
        radius: f64,
        u_dir: V3,
        pole: V3,
    },
    /// `major` from the center to the tube's center line, `minor` the tube radius.
    Torus {
        center: V3,
        axis: V3,
        major: f64,
        minor: f64,
        u_dir: V3,
    },
}

/// A curve an edge lies on.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// Parameter: distance from `root` along the unit `dir`.
    Line { root: V3, dir: V3 },
    /// Parameter: angle in radians from `u_dir`, counterclockwise about `normal`.
    Circle {
        center: V3,
        normal: V3,
        u_dir: V3,
        radius: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub curve: Curve,
    pub start: usize,
    pub end: usize,
    /// Curve parameters at the start and end vertex (`t0 < t1`).
    pub t0: f64,
    pub t1: f64,
}

/// One use of an edge by a loop; `reversed` when the loop runs against the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coedge {
    pub edge: usize,
    pub reversed: bool,
}

/// A closed chain of coedges. Seen from the side the face normal points to, the face
/// lies to the left of each coedge.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loop {
    pub coedges: Vec<Coedge>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub surface: Surface,
    /// The face normal is the surface normal reversed.
    pub reversed: bool,
    /// No loops: the whole (closed) surface, e.g. a full sphere.
    pub loops: Vec<Loop>,
}

/// A body with one lump and one shell. `solid`: a closed shell (single-sided faces);
/// otherwise a sheet (double-sided faces), which AutoCAD shows as a surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub vertices: Vec<V3>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub solid: bool,
}

impl Surface {
    /// Signed distance from `p` to the surface (positive on the side the surface
    /// normal points to).
    pub fn distance(&self, p: V3) -> f64 {
        match *self {
            Surface::Plane { root, normal, .. } => dot(sub(p, root), normal),
            Surface::Cone {
                center,
                axis,
                radius,
                slope,
                ..
            } => {
                let v = sub(p, center);
                let h = dot(v, axis);
                let rho = norm(sub(v, scale(axis, h)));
                // Perpendicular distance to the generating line at this height.
                (rho - (radius + slope * h)) / (1.0 + slope * slope).sqrt()
            }
            Surface::Sphere { center, radius, .. } => norm(sub(p, center)) - radius,
            Surface::Torus {
                center,
                axis,
                major,
                minor,
                ..
            } => {
                let v = sub(p, center);
                let z = dot(v, axis);
                let rho = norm(sub(v, scale(axis, z)));
                ((rho - major).powi(2) + z * z).sqrt() - minor
            }
        }
    }

    /// Outward surface normal at (or nearest to) `p`.
    pub fn normal(&self, p: V3) -> V3 {
        match *self {
            Surface::Plane { normal, .. } => normal,
            Surface::Cone {
                center,
                axis,
                u_dir,
                slope,
                ..
            } => {
                let v = sub(p, center);
                let radial = sub(v, scale(axis, dot(v, axis)));
                let r = if norm(radial) > 0.0 {
                    unit(radial)
                } else {
                    u_dir
                };
                unit(sub(r, scale(axis, slope)))
            }
            Surface::Sphere { center, .. } => unit(sub(p, center)),
            Surface::Torus {
                center,
                axis,
                major,
                u_dir,
                ..
            } => {
                let v = sub(p, center);
                let radial = sub(v, scale(axis, dot(v, axis)));
                let r = if norm(radial) > 0.0 {
                    unit(radial)
                } else {
                    u_dir
                };
                unit(sub(p, add(center, scale(r, major))))
            }
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "plane",
            Surface::Cone { slope, .. } if *slope == 0.0 => "cylinder",
            Surface::Cone { .. } => "cone",
            Surface::Sphere { .. } => "sphere",
            Surface::Torus { .. } => "torus",
        }
    }
}

impl Curve {
    pub fn point(&self, t: f64) -> V3 {
        match *self {
            Curve::Line { root, dir } => add(root, scale(dir, t)),
            Curve::Circle {
                center,
                normal,
                u_dir,
                radius,
            } => {
                let v_dir = cross(normal, u_dir);
                add(
                    center,
                    add(
                        scale(u_dir, radius * t.cos()),
                        scale(v_dir, radius * t.sin()),
                    ),
                )
            }
        }
    }

    /// Distance from `p` to the (infinite) curve.
    pub fn distance(&self, p: V3) -> f64 {
        match *self {
            Curve::Line { root, dir } => norm(cross(sub(p, root), dir)),
            Curve::Circle {
                center,
                normal,
                radius,
                ..
            } => {
                let v = sub(p, center);
                let z = dot(v, normal);
                let rho = norm(sub(v, scale(normal, z)));
                ((rho - radius).powi(2) + z * z).sqrt()
            }
        }
    }
}

/// Why a body failed [`Body::validate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid(pub String);

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Body {
    /// Check that the geometry and topology agree to within `tol` (model units).
    pub fn validate(&self, tol: f64) -> Result<(), Invalid> {
        let bad = |m: String| Err(Invalid(m));
        for (i, e) in self.edges.iter().enumerate() {
            if e.t0.partial_cmp(&e.t1) != Some(std::cmp::Ordering::Less)
                || e.start >= self.vertices.len()
                || e.end >= self.vertices.len()
            {
                return bad(format!("edge {i}: bad parameters or vertices"));
            }
            for (t, v) in [(e.t0, e.start), (e.t1, e.end)] {
                let d = dist(e.curve.point(t), self.vertices[v]);
                if d > tol {
                    return bad(format!("edge {i}: vertex {v} is {d:e} from its curve end"));
                }
            }
        }
        for (fi, f) in self.faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                if l.coedges.is_empty() {
                    return bad(format!("face {fi} loop {li} is empty"));
                }
                let ends = |c: &Coedge| {
                    let e = &self.edges[c.edge];
                    if c.reversed {
                        (e.end, e.start)
                    } else {
                        (e.start, e.end)
                    }
                };
                for (k, c) in l.coedges.iter().enumerate() {
                    if c.edge >= self.edges.len() {
                        return bad(format!("face {fi} loop {li}: no edge {}", c.edge));
                    }
                    let next = &l.coedges[(k + 1) % l.coedges.len()];
                    if ends(c).1 != ends(next).0 {
                        return bad(format!("face {fi} loop {li} does not close at coedge {k}"));
                    }
                    let e = &self.edges[c.edge];
                    for s in 0..=16 {
                        let p = e.curve.point(e.t0 + (e.t1 - e.t0) * f64::from(s) / 16.0);
                        let d = f.surface.distance(p).abs();
                        if d > tol {
                            return bad(format!(
                                "face {fi}: edge {} leaves the surface by {d:e}",
                                c.edge
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl Body {
    /// Axis-aligned bounds: vertices, points along edges, and closed surfaces.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let mut pts: Vec<V3> = self.vertices.clone();
        for e in &self.edges {
            for s in 0..=32 {
                pts.push(e.curve.point(e.t0 + (e.t1 - e.t0) * f64::from(s) / 32.0));
            }
        }
        for f in self.faces.iter().filter(|f| f.loops.is_empty()) {
            let (c, r) = match f.surface {
                Surface::Sphere { center, radius, .. } => (center, radius),
                Surface::Torus {
                    center,
                    major,
                    minor,
                    ..
                } => (center, major + minor),
                _ => continue,
            };
            pts.push(sub(c, [r; 3]));
            pts.push(add(c, [r; 3]));
        }
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
                [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
            )
        }))
    }
}
