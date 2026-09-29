//! A region's boundary as lines and circular arcs that lie on its surface, and the
//! resulting ACIS body. A boundary that isn't made of such curves (where two curved
//! surfaces meet in a general curve) makes the region stay faceted.

use crate::mesh::Mesh;
use crate::prim::{circle3, Prim};
use obj2cad_acis::builder::{ccw_angle, Builder, Seg};
use obj2cad_acis::*;
use std::collections::HashMap;

/// Closed boundary loops of `faces`, each as welded vertex ids in the direction the
/// faces' winding gives them (face on the left). `None` for non-manifold or
/// inconsistently wound regions.
pub fn loops(mesh: &Mesh, faces: &[u32]) -> Option<Vec<Vec<u32>>> {
    let mut count: HashMap<(u32, u32), (u32, u32, u32)> = HashMap::new(); // key → (uses, a, b)
    for &f in faces {
        let idx = mesh.face(f as usize);
        for k in 0..idx.len() {
            let (a, b) = (idx[k], idx[(k + 1) % idx.len()]);
            if a == b {
                continue;
            }
            let e = count.entry((a.min(b), a.max(b))).or_insert((0, a, b));
            e.0 += 1;
            if e.0 == 2 && (e.1, e.2) == (a, b) {
                return None; // both faces run the edge the same way
            }
        }
    }
    let mut next: HashMap<u32, u32> = HashMap::new();
    for &(uses, a, b) in count.values() {
        match uses {
            1 => {
                if next.insert(a, b).is_some() {
                    return None; // two boundary edges leave one vertex
                }
            }
            2 => {}
            _ => return None,
        }
    }
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut seen: HashMap<u32, bool> = HashMap::new();
    let mut out = Vec::new();
    for s in starts {
        if seen.contains_key(&s) {
            continue;
        }
        let mut l = vec![s];
        seen.insert(s, true);
        let mut v = *next.get(&s)?;
        while v != s {
            if seen.insert(v, true).is_some() {
                return None;
            }
            l.push(v);
            v = *next.get(&v)?;
        }
        out.push(l);
    }
    Some(out)
}

/// Does the circle through (center, normal, radius) stay on the surface from `from`
/// counterclockwise by `angle`?
fn arc_on_surface(prim: &Prim, center: V3, normal: V3, from: V3, angle: f64, tol: f64) -> bool {
    let u = unit(sub(from, center));
    let v = cross(normal, u);
    let r = dist(from, center);
    (0..=24).all(|k| {
        let t = angle * f64::from(k) / 24.0;
        let p = add(center, add(scale(u, r * t.cos()), scale(v, r * t.sin())));
        prim.distance(p).abs() <= tol
    })
}

/// Points `pts` (in order) as one straight line on the surface.
fn as_line(prim: &Prim, pts: &[V3], tols: &[f64], tol: f64) -> Option<Seg> {
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let d = unit(sub(b, a));
    if dist(a, b) == 0.0 {
        return None;
    }
    let on_line = pts
        .iter()
        .zip(tols)
        .all(|(&p, &t)| norm(cross(sub(p, a), d)) <= t);
    let on_surface = (0..=8).all(|k| {
        prim.distance(add(a, scale(sub(b, a), f64::from(k) / 8.0)))
            .abs()
            <= tol
    });
    (on_line && on_surface).then_some(Seg::Line { from: a, to: b })
}

/// Points `pts` (in order, at least 3) as one arc on the surface; a full circle when
/// `closed` (the first point is repeated at the end implicitly).
fn as_arc(prim: &Prim, pts: &[V3], tols: &[f64], tol: f64, closed: bool) -> Option<Seg> {
    if pts.len() < 3 {
        return None;
    }
    let (center, mut normal, r) = circle3(pts)?;
    // Traversal direction: counterclockwise about `normal`.
    let mid = pts[pts.len() / 2];
    if dot(
        cross(sub(mid, pts[0]), sub(pts[pts.len() - 1], mid)),
        normal,
    ) < 0.0
        || closed && dot(cross(sub(pts[1], center), sub(pts[2], center)), normal) < 0.0
    {
        normal = scale(normal, -1.0);
    }
    // Each point on the circle, and angles increasing monotonically.
    let on_circle = pts.iter().zip(tols).all(|(&p, &t)| {
        let v = sub(p, center);
        let z = dot(v, normal);
        ((norm(sub(v, scale(normal, z))) - r).powi(2) + z * z).sqrt() <= t
    });
    if !on_circle {
        return None;
    }
    let u0 = sub(pts[0], center);
    let mut last = 0.0;
    for &p in &pts[1..] {
        let a = ccw_angle(u0, sub(p, center), normal);
        if a <= last {
            return None;
        }
        last = a;
    }
    let total = if closed { std::f64::consts::TAU } else { last };
    // Snap the arc's ends onto exactly these points (the builder uses them as vertices).
    let seg = Seg::Arc {
        center,
        normal,
        from: pts[0],
        to: if closed { pts[0] } else { pts[pts.len() - 1] },
    };
    arc_on_surface(prim, center, normal, pts[0], total, tol).then_some(seg)
}

/// A closed loop of points as line and arc segments on the surface.
fn segments(prim: &Prim, pts: &[V3], tols: &[f64], tol: f64) -> Option<Vec<Seg>> {
    let n = pts.len();
    if n >= 3 {
        if let Some(s) = as_arc(prim, pts, tols, tol, true) {
            return Some(vec![s]);
        }
    }
    let at = |i: usize| pts[i % n];
    let tol_at = |i: usize| tols[i % n];
    // Longest run from `i` that is one line or one arc.
    let run = |i: usize, limit: usize| -> Option<(usize, Seg)> {
        let mut best: Option<(usize, Seg)> = None;
        for j in i + 1..=i + limit {
            let p: Vec<V3> = (i..=j).map(at).collect();
            let t: Vec<f64> = (i..=j).map(tol_at).collect();
            let seg = as_line(prim, &p, &t, tol).or_else(|| as_arc(prim, &p, &t, tol, false));
            match seg {
                Some(s) => best = Some((j, s)),
                None if j >= i + 3 => break, // longer runs won't fit either
                None => {}
            }
        }
        best
    };
    let greedy = |start: usize| -> Option<Vec<(usize, Seg)>> {
        let mut out = Vec::new();
        let mut i = start;
        while i < start + n {
            let (j, s) = run(i, start + n - i)?;
            out.push((j, s));
            i = j;
        }
        Some(out)
    };
    // Start where a first pass found a break, so no run straddles the start.
    let first = greedy(0)?;
    let start = first.first().map_or(0, |r| r.0 % n);
    let segs = greedy(start)?;
    Some(segs.into_iter().map(|(_, s)| s).collect())
}

/// The sheet body of a region: its surface bounded by its loops. `reversed`: the mesh's
/// faces point against the surface's outward normal.
pub fn body(mesh: &Mesh, faces: &[u32], prim: &Prim, reversed: bool, tol: f64) -> Option<Body> {
    let loops = loops(mesh, faces)?;
    let near = mesh.center[faces[0] as usize];
    let surface = prim.surface(near);
    let mut segs: Vec<Vec<Seg>> = Vec::new();
    for l in &loops {
        let pts: Vec<V3> = l.iter().map(|&v| mesh.pos[v as usize]).collect();
        let tols: Vec<f64> = l
            .iter()
            .map(|&v| mesh.tol[v as usize].max(tol * 1e-3))
            .collect();
        segs.push(segments(prim, &pts, &tols, tol)?);
    }
    let closed = loops.is_empty();
    let mut b = Builder::new(closed);
    b.face(surface, reversed, &segs);
    let body = b.finish();
    body.validate(tol * 4.0).ok()?;
    Some(body)
}
