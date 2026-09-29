//! A body as ACIS records, independent of SAT/SAB encoding.

use crate::{Body, Curve, Surface, V3};

#[derive(Clone)]
pub(crate) enum Tok {
    Ptr(i64),
    /// An integer only SAB carries (vertex and coedge have one; SAT omits it).
    SabInt(i32),
    Double(f64),
    /// A position (SAB tag 0x13).
    Pos(V3),
    /// A direction (SAB tag 0x14).
    Dir(V3),
    /// A two-valued keyword: (value, text when true, text when false).
    Bool(bool, &'static str, &'static str),
    /// `I` (infinite) or `F value`.
    Interval(Option<f64>),
    Str(&'static str),
}

pub(crate) struct Record {
    pub name: &'static str,
    pub toks: Vec<Tok>,
}

const NULL: i64 = -1;
const INF: Tok = Tok::Interval(None);

fn sense(reversed: bool) -> Tok {
    Tok::Bool(reversed, "reversed", "forward")
}

/// The records of `body`, in pointer order: (optional ASM header,) body, lump, shell,
/// faces, loops, coedges, edges, vertices, points, surfaces, curves.
pub(crate) fn records(body: &Body, asm_header: Option<&'static str>) -> Vec<Record> {
    use Tok::*;
    let mut out: Vec<Record> = Vec::new();
    let base = i64::from(asm_header.is_some());
    if let Some(v) = asm_header {
        out.push(Record {
            name: "asmheader",
            toks: vec![Str(v)],
        });
    }
    // Index layout.
    let n_faces = body.faces.len() as i64;
    let n_loops: i64 = body.faces.iter().map(|f| f.loops.len() as i64).sum();
    let n_coedges: i64 = body
        .faces
        .iter()
        .flat_map(|f| &f.loops)
        .map(|l| l.coedges.len() as i64)
        .sum();
    let n_edges = body.edges.len() as i64;
    let n_vertices = body.vertices.len() as i64;
    let (i_body, i_lump, i_shell) = (base, base + 1, base + 2);
    let i_face = i_shell + 1;
    let i_loop = i_face + n_faces;
    let i_coedge = i_loop + n_loops;
    let i_edge = i_coedge + n_coedges;
    let i_vertex = i_edge + n_edges;
    let i_point = i_vertex + n_vertices;
    let i_surface = i_point + n_vertices;
    let i_curve = i_surface + n_faces;

    // Coedges in order, with their loop, and each edge's coedges (for partners).
    struct Co {
        edge: usize,
        reversed: bool,
        loop_: i64,
        next: i64,
        prev: i64,
    }
    let mut cos: Vec<Co> = Vec::new();
    let mut loop_first: Vec<i64> = Vec::new();
    let mut loop_face: Vec<i64> = Vec::new();
    let mut face_first_loop: Vec<i64> = Vec::new();
    for (fi, f) in body.faces.iter().enumerate() {
        face_first_loop.push(if f.loops.is_empty() {
            NULL
        } else {
            i_loop + loop_first.len() as i64
        });
        for l in &f.loops {
            let li = i_loop + loop_first.len() as i64;
            let start = cos.len() as i64;
            let n = l.coedges.len() as i64;
            loop_first.push(i_coedge + start);
            loop_face.push(i_face + fi as i64);
            for (k, c) in l.coedges.iter().enumerate() {
                let k = k as i64;
                cos.push(Co {
                    edge: c.edge,
                    reversed: c.reversed,
                    loop_: li,
                    next: i_coedge + start + (k + 1) % n,
                    prev: i_coedge + start + (k + n - 1) % n,
                });
            }
        }
    }
    let mut by_edge: Vec<Vec<i64>> = vec![Vec::new(); body.edges.len()];
    for (k, c) in cos.iter().enumerate() {
        by_edge[c.edge].push(i_coedge + k as i64);
    }
    let mut vertex_edge: Vec<i64> = vec![NULL; body.vertices.len()];
    for (i, e) in body.edges.iter().enumerate() {
        for v in [e.start, e.end] {
            if vertex_edge[v] == NULL {
                vertex_edge[v] = i_edge + i as i64;
            }
        }
    }

    out.push(Record {
        name: "body",
        toks: vec![Ptr(NULL), Ptr(i_lump), Ptr(NULL), Ptr(NULL)],
    });
    out.push(Record {
        name: "lump",
        toks: vec![Ptr(NULL), Ptr(NULL), Ptr(i_shell), Ptr(i_body)],
    });
    out.push(Record {
        name: "shell",
        toks: vec![
            Ptr(NULL),
            Ptr(NULL),
            Ptr(NULL),
            Ptr(if n_faces > 0 { i_face } else { NULL }),
            Ptr(NULL),
            Ptr(i_lump),
        ],
    });
    for (fi, f) in body.faces.iter().enumerate() {
        let next = if (fi as i64) + 1 < n_faces {
            i_face + fi as i64 + 1
        } else {
            NULL
        };
        let mut toks = vec![
            Ptr(NULL),
            Ptr(next),
            Ptr(face_first_loop[fi]),
            Ptr(i_shell),
            Ptr(NULL),
            Ptr(i_surface + fi as i64),
            sense(f.reversed),
            Bool(!body.solid, "double", "single"),
        ];
        if !body.solid {
            toks.push(Bool(false, "in", "out"));
        }
        out.push(Record { name: "face", toks });
    }
    for (li, &first) in loop_first.iter().enumerate() {
        // The next loop of the same face, if any.
        let next = if li + 1 < loop_face.len() && loop_face[li + 1] == loop_face[li] {
            i_loop + li as i64 + 1
        } else {
            NULL
        };
        out.push(Record {
            name: "loop",
            toks: vec![Ptr(NULL), Ptr(next), Ptr(first), Ptr(loop_face[li])],
        });
    }
    for (k, c) in cos.iter().enumerate() {
        let me = i_coedge + k as i64;
        let group = &by_edge[c.edge];
        let partner = if group.len() < 2 {
            NULL
        } else {
            let at = group.iter().position(|&x| x == me).expect("listed");
            group[(at + 1) % group.len()]
        };
        out.push(Record {
            name: "coedge",
            toks: vec![
                Ptr(NULL),
                Ptr(c.next),
                Ptr(c.prev),
                Ptr(partner),
                Ptr(i_edge + c.edge as i64),
                sense(c.reversed),
                Ptr(c.loop_),
                SabInt(0),
                Ptr(NULL),
            ],
        });
    }
    for (i, e) in body.edges.iter().enumerate() {
        out.push(Record {
            name: "edge",
            toks: vec![
                Ptr(NULL),
                Ptr(i_vertex + e.start as i64),
                Double(e.t0),
                Ptr(i_vertex + e.end as i64),
                Double(e.t1),
                Ptr(by_edge[i].first().copied().unwrap_or(NULL)),
                Ptr(i_curve + i as i64),
                sense(false),
                Str("unknown"),
            ],
        });
    }
    for (i, _) in body.vertices.iter().enumerate() {
        out.push(Record {
            name: "vertex",
            toks: vec![
                Ptr(NULL),
                Ptr(vertex_edge[i]),
                SabInt(0),
                Ptr(i_point + i as i64),
            ],
        });
    }
    for &p in &body.vertices {
        out.push(Record {
            name: "point",
            toks: vec![Ptr(NULL), Pos(p)],
        });
    }
    for f in &body.faces {
        out.push(surface(&f.surface));
    }
    for e in &body.edges {
        out.push(curve(&e.curve));
    }
    out
}

fn surface(s: &Surface) -> Record {
    use Tok::*;
    let tail = [Bool(false, "reverse_v", "forward_v"), INF, INF, INF, INF];
    match *s {
        Surface::Plane {
            root,
            normal,
            u_dir,
        } => Record {
            name: "plane-surface",
            toks: [
                vec![Ptr(NULL), Pos(root), Dir(normal), Dir(u_dir)],
                tail.into(),
            ]
            .concat(),
        },
        Surface::Cone {
            center,
            axis,
            u_dir,
            radius,
            slope,
        } => {
            let c = 1.0 / (1.0 + slope * slope).sqrt();
            Record {
                name: "cone-surface",
                toks: vec![
                    Ptr(NULL),
                    Pos(center),
                    Dir(axis),
                    Dir(crate::scale(u_dir, radius)),
                    Double(1.0),
                    INF,
                    INF,
                    Double(slope * c),
                    Double(c),
                    Double(radius),
                    sense(false),
                    INF,
                    INF,
                    INF,
                    INF,
                ],
            }
        }
        Surface::Sphere {
            center,
            radius,
            u_dir,
            pole,
        } => Record {
            name: "sphere-surface",
            toks: [
                vec![
                    Ptr(NULL),
                    Pos(center),
                    Double(radius),
                    Dir(u_dir),
                    Dir(pole),
                ],
                tail.into(),
            ]
            .concat(),
        },
        Surface::Torus {
            center,
            axis,
            major,
            minor,
            u_dir,
        } => Record {
            name: "torus-surface",
            toks: [
                vec![
                    Ptr(NULL),
                    Pos(center),
                    Dir(axis),
                    Double(major),
                    Double(minor),
                    Dir(u_dir),
                ],
                tail.into(),
            ]
            .concat(),
        },
    }
}

fn curve(c: &Curve) -> Record {
    use Tok::*;
    match *c {
        Curve::Line { root, dir } => Record {
            name: "straight-curve",
            toks: vec![Ptr(NULL), Pos(root), Dir(dir), INF, INF],
        },
        Curve::Circle {
            center,
            normal,
            u_dir,
            radius,
        } => Record {
            name: "ellipse-curve",
            toks: vec![
                Ptr(NULL),
                Pos(center),
                Dir(normal),
                Dir(crate::scale(u_dir, radius)),
                Double(1.0),
                INF,
                INF,
            ],
        },
    }
}

/// `Www Mmm dd hh:mm:ss yyyy` (24 characters) for Unix seconds, in UTC.
pub(crate) fn ctime(unix: f64) -> String {
    let secs = if unix.is_finite() {
        unix.floor() as i64
    } else {
        0
    };
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const WD: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MO: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {:02} {:02}:{:02}:{:02} {:04}",
        WD[days.rem_euclid(7) as usize],
        MO[(month - 1) as usize],
        day,
        sod / 3600,
        sod % 3600 / 60,
        sod % 60,
        year
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn ctime_matches_the_c_library() {
        assert_eq!(super::ctime(0.0), "Thu Jan 01 00:00:00 1970");
        assert_eq!(super::ctime(1_790_000_000.0), "Mon Sep 21 14:13:20 2026");
        assert_eq!(super::ctime(951_782_400.0), "Tue Feb 29 00:00:00 2000");
    }
}
