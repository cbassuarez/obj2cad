//! The surfaces obj2cad recognizes: fitting, distances, and conversion to ACIS.

use crate::linalg::{eigen_sym, least_squares, levenberg_marquardt, M3};
use obj2cad_acis::*;
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prim {
    Sphere {
        c: V3,
        r: f64,
    },
    /// `p` is any point on the axis.
    Cylinder {
        p: V3,
        a: V3,
        r: f64,
    },
    /// Points lie at positive heights along `a` from the apex; `angle` is the half-angle.
    Cone {
        apex: V3,
        a: V3,
        angle: f64,
    },
    Torus {
        c: V3,
        a: V3,
        major: f64,
        minor: f64,
    },
}

fn radial(v: V3, a: V3) -> V3 {
    sub(v, scale(a, dot(v, a)))
}

/// Unit vector `a` tilted by (d0, d1) in its own tangent plane.
fn tilt(a: V3, d0: f64, d1: f64) -> V3 {
    let e1 = perpendicular(a);
    let e2 = cross(a, e1);
    unit(add(a, add(scale(e1, d0), scale(e2, d1))))
}

impl Prim {
    pub fn kind(&self) -> &'static str {
        match self {
            Prim::Sphere { .. } => "sphere",
            Prim::Cylinder { .. } => "cylinder",
            Prim::Cone { .. } => "cone",
            Prim::Torus { .. } => "torus",
        }
    }

    pub fn params(&self) -> usize {
        match self {
            Prim::Sphere { .. } => 4,
            Prim::Cylinder { .. } => 5,
            Prim::Cone { .. } => 6,
            Prim::Torus { .. } => 7,
        }
    }

    /// Signed distance (positive outside: away from the center or axis).
    pub fn distance(&self, x: V3) -> f64 {
        match *self {
            Prim::Sphere { c, r } => dist(x, c) - r,
            Prim::Cylinder { p, a, r } => norm(radial(sub(x, p), a)) - r,
            Prim::Cone { apex, a, angle } => {
                let v = sub(x, apex);
                let h = dot(v, a);
                norm(radial(v, a)) * angle.cos() - h * angle.sin()
            }
            Prim::Torus { c, a, major, minor } => {
                let v = sub(x, c);
                let z = dot(v, a);
                ((norm(radial(v, a)) - major).powi(2) + z * z).sqrt() - minor
            }
        }
    }

    /// Outward normal at the point of the surface nearest `x`.
    pub fn normal(&self, x: V3) -> V3 {
        match *self {
            Prim::Sphere { c, .. } => unit(sub(x, c)),
            Prim::Cylinder { p, a, .. } => unit(radial(sub(x, p), a)),
            Prim::Cone { apex, a, angle } => {
                let r = unit(radial(sub(x, apex), a));
                unit(sub(scale(r, angle.cos()), scale(a, angle.sin())))
            }
            Prim::Torus { c, a, major, .. } => {
                let r = unit(radial(sub(x, c), a));
                unit(sub(x, add(c, scale(r, major))))
            }
        }
    }

    /// This surface moved by local parameters `d` (see `params`).
    pub fn moved(&self, d: &[f64]) -> Prim {
        match *self {
            Prim::Sphere { c, r } => Prim::Sphere {
                c: add(c, [d[0], d[1], d[2]]),
                r: r + d[3],
            },
            Prim::Cylinder { p, a, r } => {
                let e1 = perpendicular(a);
                let e2 = cross(a, e1);
                Prim::Cylinder {
                    p: add(p, add(scale(e1, d[2]), scale(e2, d[3]))),
                    a: tilt(a, d[0], d[1]),
                    r: r + d[4],
                }
            }
            Prim::Cone { apex, a, angle } => Prim::Cone {
                apex: add(apex, [d[0], d[1], d[2]]),
                a: tilt(a, d[3], d[4]),
                angle: angle + d[5],
            },
            Prim::Torus { c, a, major, minor } => Prim::Torus {
                c: add(c, [d[0], d[1], d[2]]),
                a: tilt(a, d[3], d[4]),
                major: major + d[5],
                minor: minor + d[6],
            },
        }
    }

    /// Difference steps for each local parameter, for a model of size `size`.
    fn steps(&self, size: f64) -> Vec<f64> {
        let (l, ang) = (size * 1e-7, 1e-7);
        match self {
            Prim::Sphere { .. } => vec![l, l, l, l],
            Prim::Cylinder { .. } => vec![ang, ang, l, l, l],
            Prim::Cone { .. } => vec![l, l, l, ang, ang, ang],
            Prim::Torus { .. } => vec![l, l, l, ang, ang, l, l],
        }
    }

    fn sane(&self) -> bool {
        let ok = |v: f64| v.is_finite();
        match *self {
            Prim::Sphere { c, r } => c.iter().all(|&v| ok(v)) && r > 0.0 && ok(r),
            Prim::Cylinder { p, a, r } => p.iter().chain(&a).all(|&v| ok(v)) && r > 0.0 && ok(r),
            Prim::Cone { apex, a, angle } => {
                apex.iter().chain(&a).all(|&v| ok(v)) && angle > 1e-4 && angle < 1.5
            }
            Prim::Torus { c, a, major, minor } => {
                c.iter().chain(&a).all(|&v| ok(v))
                    && minor > 0.0
                    && major > 0.0
                    && ok(major)
                    && ok(minor)
            }
        }
    }

    /// Refine by least squares on distances weighted by each point's tolerance.
    pub fn refine(&self, pts: &[V3], tols: &[f64], size: f64) -> Prim {
        let cur = Cell::new(*self);
        let n = self.params();
        levenberg_marquardt(
            n,
            60,
            |d, out| {
                out.clear();
                let m = cur.get().moved(d);
                out.extend(pts.iter().zip(tols).map(|(&p, &t)| m.distance(p) / t));
            },
            |d| cur.set(cur.get().moved(d)),
            &self.steps(size),
            0.5,
            1e3,
        );
        let m = cur.get();
        if m.sane() {
            m
        } else {
            *self
        }
    }

    /// Largest |distance| / tolerance over the points.
    pub fn worst(&self, pts: &[V3], tols: &[f64]) -> f64 {
        pts.iter()
            .zip(tols)
            .map(|(&p, &t)| (self.distance(p) / t).abs())
            .fold(0.0, f64::max)
    }

    /// The ACIS surface. `near` is a point of the region (sets the reference circle).
    pub fn surface(&self, near: V3) -> Surface {
        match *self {
            Prim::Sphere { c, r } => Surface::Sphere {
                center: c,
                radius: r,
                u_dir: [1.0, 0.0, 0.0],
                pole: [0.0, 0.0, 1.0],
            },
            Prim::Cylinder { p, a, r } => Surface::Cone {
                center: add(p, scale(a, dot(sub(near, p), a))),
                axis: a,
                u_dir: perpendicular(a),
                radius: r,
                slope: 0.0,
            },
            Prim::Cone { apex, a, angle } => {
                let h = dot(sub(near, apex), a);
                Surface::Cone {
                    center: add(apex, scale(a, h)),
                    axis: a,
                    u_dir: perpendicular(a),
                    radius: h * angle.tan(),
                    slope: angle.tan(),
                }
            }
            Prim::Torus { c, a, major, minor } => Surface::Torus {
                center: c,
                axis: a,
                major,
                minor,
                u_dir: perpendicular(a),
            },
        }
    }
}

// ---------------------------------------------------------------- initial estimates

fn centroid(pts: &[V3]) -> V3 {
    let n = pts.len() as f64;
    scale(pts.iter().fold([0.0; 3], |s, &p| add(s, p)), 1.0 / n)
}

fn covariance(vs: impl Iterator<Item = (V3, f64)>) -> M3 {
    let mut m = [[0.0; 3]; 3];
    for (v, w) in vs {
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += w * v[i] * v[j];
            }
        }
    }
    m
}

/// Best-fit plane: (point, unit normal, largest |distance|).
pub fn plane(pts: &[V3]) -> (V3, V3, f64) {
    let c = centroid(pts);
    let (_, vecs) = eigen_sym(covariance(pts.iter().map(|&p| (sub(p, c), 1.0))));
    let n = vecs[0];
    let worst = pts
        .iter()
        .map(|&p| dot(sub(p, c), n).abs())
        .fold(0.0, f64::max);
    (c, n, worst)
}

/// Algebraic circle through 2-D points: (center, radius).
fn circle2(pts: &[[f64; 2]]) -> Option<([f64; 2], f64)> {
    let n = pts.len() as f64;
    let m = pts
        .iter()
        .fold([0.0; 2], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n]);
    let x = least_squares(pts.iter().map(|p| {
        let (u, v) = (p[0] - m[0], p[1] - m[1]);
        ([u, v, 1.0], -(u * u + v * v))
    }))?;
    let (cu, cv) = (-x[0] / 2.0, -x[1] / 2.0);
    let r2 = cu * cu + cv * cv - x[2];
    (r2 > 0.0).then(|| ([cu + m[0], cv + m[1]], r2.sqrt()))
}

/// Circle through 3-D points: (center, unit normal, radius).
pub fn circle3(pts: &[V3]) -> Option<(V3, V3, f64)> {
    let (c, n, _) = plane(pts);
    let e1 = perpendicular(n);
    let e2 = cross(n, e1);
    let flat: Vec<[f64; 2]> = pts
        .iter()
        .map(|&p| {
            let v = sub(p, c);
            [dot(v, e1), dot(v, e2)]
        })
        .collect();
    let (cc, r) = circle2(&flat)?;
    Some((add(c, add(scale(e1, cc[0]), scale(e2, cc[1]))), n, r))
}

pub fn sphere(pts: &[V3]) -> Option<Prim> {
    let m = centroid(pts);
    let x = least_squares(pts.iter().map(|&p| {
        let v = sub(p, m);
        ([v[0], v[1], v[2], 1.0], -dot(v, v))
    }))?;
    let c = [-x[0] / 2.0, -x[1] / 2.0, -x[2] / 2.0];
    let r2 = dot(c, c) - x[3];
    (r2 > 0.0).then(|| Prim::Sphere {
        c: add(c, m),
        r: r2.sqrt(),
    })
}

/// From points and (area-weighted) face normals.
pub fn cylinder(pts: &[V3], normals: &[(V3, f64)]) -> Option<Prim> {
    let (_, vecs) = eigen_sym(covariance(normals.iter().copied()));
    let a = vecs[0];
    let e1 = perpendicular(a);
    let e2 = cross(a, e1);
    let m = centroid(pts);
    let flat: Vec<[f64; 2]> = pts
        .iter()
        .map(|&p| {
            let v = sub(p, m);
            [dot(v, e1), dot(v, e2)]
        })
        .collect();
    let (cc, r) = circle2(&flat)?;
    Some(Prim::Cylinder {
        p: add(m, add(scale(e1, cc[0]), scale(e2, cc[1]))),
        a,
        r,
    })
}

/// From points and face normals with their face centers. `None` when the normals say
/// cylinder (or nothing).
pub fn cone(pts: &[V3], normals: &[(V3, f64)], centers: &[V3]) -> Option<Prim> {
    let wsum: f64 = normals.iter().map(|x| x.1).sum();
    let mean = scale(
        normals
            .iter()
            .fold([0.0; 3], |s, &(n, w)| add(s, scale(n, w))),
        1.0 / wsum,
    );
    let (_, vecs) = eigen_sym(covariance(normals.iter().map(|&(n, w)| (sub(n, mean), w))));
    let mut a = vecs[0];
    let c = dot(mean, a);
    if c.abs() < 1e-3 || c.abs() > 0.999 {
        return None;
    }
    // Apex: every tangent plane passes through it.
    let apex = least_squares(normals.iter().zip(centers).map(|(&(n, w), &q)| {
        let s = w.sqrt();
        ([n[0] * s, n[1] * s, n[2] * s], dot(n, q) * s)
    }))?;
    let mean_h = pts.iter().map(|&p| dot(sub(p, apex), a)).sum::<f64>() / pts.len() as f64;
    if mean_h < 0.0 {
        a = scale(a, -1.0);
    }
    let angle = c.abs().asin();
    Some(Prim::Cone { apex, a, angle })
}

/// From points and their (vertex) normals: the tube's center line is a circle. The tube
/// radius is searched near `1 / curvature`, the largest normal curvature along the
/// patch's edges (`edges`: pairs of indices into `pts`).
pub fn torus(pts: &[V3], normals: &[V3], edges: &[(usize, usize)]) -> Option<Prim> {
    let kappa = edges
        .iter()
        .map(|&(i, j)| {
            let d = dist(pts[i], pts[j]);
            if d > 0.0 {
                dist(normals[i], normals[j]) / d
            } else {
                0.0
            }
        })
        .fold(0.0f64, f64::max);
    if !(kappa > 0.0 && kappa.is_finite()) {
        return None;
    }
    let fit = |r: f64, s: f64| -> Option<(f64, Prim)> {
        let q: Vec<V3> = pts
            .iter()
            .zip(normals)
            .map(|(&p, &n)| sub(p, scale(n, s * r)))
            .collect();
        let (c, n, r_major) = circle3(&q)?;
        let t = Prim::Torus {
            c,
            a: n,
            major: r_major,
            minor: r,
        };
        let cost: f64 = pts.iter().map(|&p| t.distance(p).powi(2)).sum();
        (r_major > 0.0 && cost.is_finite()).then_some((cost, t))
    };
    let mut best: Option<(f64, Prim, f64, f64)> = None;
    for s in [1.0, -1.0] {
        for k in 0..9 {
            let r = (0.5 * 4f64.powf(f64::from(k) / 8.0)) / kappa;
            if let Some((cost, t)) = fit(r, s) {
                if best.as_ref().is_none_or(|b| cost < b.0) {
                    best = Some((cost, t, r, s));
                }
            }
        }
    }
    let (_, mut t, r0, s) = best?;
    let (mut lo, mut hi) = (r0 / 1.2, r0 * 1.2);
    for _ in 0..16 {
        let m1 = hi - (hi - lo) / 1.618;
        let m2 = lo + (hi - lo) / 1.618;
        let (c1, c2) = (
            fit(m1, s).map_or(f64::INFINITY, |x| x.0),
            fit(m2, s).map_or(f64::INFINITY, |x| x.0),
        );
        if c1 < c2 {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    if let Some((_, tt)) = fit((lo + hi) / 2.0, s) {
        t = tt;
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_are_zero_on_the_surfaces() {
        let cyl = Prim::Cylinder {
            p: [1.0, 2.0, 3.0],
            a: [0.0, 0.0, 1.0],
            r: 5.0,
        };
        assert!(cyl.distance([6.0, 2.0, 100.0]).abs() < 1e-12);
        let cone = Prim::Cone {
            apex: [0.0; 3],
            a: [0.0, 0.0, 1.0],
            angle: 0.5,
        };
        let h: f64 = 7.0;
        assert!(cone.distance([h * 0.5f64.tan(), 0.0, h]).abs() < 1e-12);
        let tor = Prim::Torus {
            c: [0.0; 3],
            a: [0.0, 0.0, 1.0],
            major: 10.0,
            minor: 2.0,
        };
        assert!(tor.distance([12.0, 0.0, 0.0]).abs() < 1e-12);
        assert!(tor.distance([10.0, 0.0, 2.0]).abs() < 1e-12);
    }

    #[test]
    fn fits_recover_known_surfaces() {
        // Points on a sphere.
        let pts: Vec<V3> = (0..40)
            .map(|i| {
                let (t, u) = (f64::from(i) * 0.7, f64::from(i) * 0.31);
                [
                    3.0 + 4.0 * t.cos() * u.sin(),
                    -1.0 + 4.0 * t.sin() * u.sin(),
                    2.0 + 4.0 * u.cos(),
                ]
            })
            .collect();
        let Prim::Sphere { c, r } = sphere(&pts).unwrap() else {
            panic!()
        };
        assert!(dist(c, [3.0, -1.0, 2.0]) < 1e-9 && (r - 4.0).abs() < 1e-9);
    }
}
