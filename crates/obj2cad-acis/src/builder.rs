//! Build bodies from loops of line and arc segments.

use crate::*;
use std::f64::consts::TAU;

/// One piece of a loop, in the direction the loop runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line {
        from: V3,
        to: V3,
    },
    /// Counterclockwise about `normal` from `from` to `to` (a full circle when equal).
    Arc {
        center: V3,
        normal: V3,
        from: V3,
        to: V3,
    },
}

impl Seg {
    pub fn from(&self) -> V3 {
        match *self {
            Seg::Line { from, .. } | Seg::Arc { from, .. } => from,
        }
    }

    pub fn to(&self) -> V3 {
        match *self {
            Seg::Line { to, .. } | Seg::Arc { to, .. } => to,
        }
    }
}

/// Angle from `a` to `b` counterclockwise about `n`, in (0, 2π] (2π when equal).
pub fn ccw_angle(a: V3, b: V3, n: V3) -> f64 {
    let t = cross(a, b);
    let ang = dot(t, n).atan2(dot(a, b));
    if a == b {
        TAU
    } else if ang <= 0.0 {
        ang + TAU
    } else {
        ang
    }
}

#[derive(Default)]
pub struct Builder {
    body: Option<Body>,
}

impl Builder {
    pub fn new(solid: bool) -> Self {
        Self {
            body: Some(Body {
                vertices: Vec::new(),
                edges: Vec::new(),
                faces: Vec::new(),
                solid,
            }),
        }
    }

    fn b(&mut self) -> &mut Body {
        self.body.as_mut().expect("builder used after finish")
    }

    fn vertex(&mut self, p: V3) -> usize {
        let b = self.b();
        match b.vertices.iter().position(|&q| q == p) {
            Some(i) => i,
            None => {
                b.vertices.push(p);
                b.vertices.len() - 1
            }
        }
    }

    /// The edge for `seg`, reusing one another face already made (run the other way).
    fn edge(&mut self, seg: Seg) -> Coedge {
        let (a, z) = (self.vertex(seg.from()), self.vertex(seg.to()));
        let edge = match seg {
            Seg::Line { from, to } => Edge {
                curve: Curve::Line {
                    root: from,
                    dir: unit(sub(to, from)),
                },
                start: a,
                end: z,
                t0: 0.0,
                t1: dist(from, to),
            },
            Seg::Arc {
                center,
                normal,
                from,
                to,
            } => {
                let (ra, rb) = (sub(from, center), sub(to, center));
                let radius = norm(ra);
                Edge {
                    curve: Curve::Circle {
                        center,
                        normal,
                        u_dir: unit(ra),
                        radius,
                    },
                    start: a,
                    end: z,
                    t0: 0.0,
                    t1: ccw_angle(ra, rb, normal),
                }
            }
        };
        let b = self.b();
        // The same edge run backwards by another face.
        let same_geometry = |e: &Edge| match (&e.curve, &edge.curve) {
            (Curve::Line { .. }, Curve::Line { .. }) => true,
            (
                Curve::Circle {
                    center: c1,
                    normal: n1,
                    radius: r1,
                    ..
                },
                Curve::Circle {
                    center: c2,
                    normal: n2,
                    radius: r2,
                    ..
                },
            ) => c1 == c2 && r1 == r2 && dot(*n1, *n2) < 0.0,
            _ => false,
        };
        if let Some(i) = b
            .edges
            .iter()
            .position(|e| e.start == z && e.end == a && same_geometry(e))
        {
            return Coedge {
                edge: i,
                reversed: true,
            };
        }
        b.edges.push(edge);
        Coedge {
            edge: b.edges.len() - 1,
            reversed: false,
        }
    }

    pub fn face(&mut self, surface: Surface, reversed: bool, loops: &[Vec<Seg>]) {
        let loops = loops
            .iter()
            .map(|segs| Loop {
                coedges: segs.iter().map(|&s| self.edge(s)).collect(),
            })
            .collect();
        self.b().faces.push(Face {
            surface,
            reversed,
            loops,
        });
    }

    pub fn finish(mut self) -> Body {
        self.body.take().expect("finished once")
    }
}
