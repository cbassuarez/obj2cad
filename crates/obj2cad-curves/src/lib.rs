//! Recognize curved surfaces in a mesh, strictly.
//!
//! A mesh exported from CAD has its vertices *on* the original surfaces, to the precision
//! the file writes them with. This crate finds regions of faces whose every vertex lies
//! on one cylinder, cone, sphere or torus within that vertex's own stated precision (see
//! [`mesh::quantum`]; never looser than a millionth of the model's size), and whose
//! boundary is made of lines and circles on that surface. Each such region becomes an
//! ACIS surface, written next to the unchanged mesh. Everything else stays as it is.
//!
//! Scans and sculpts are noisier than their stated precision, so they rarely have such
//! regions: nothing is ever approximated to make a surface fit.

mod boundary;
pub mod linalg;
pub mod mesh;
pub mod prim;

use mesh::Mesh;
use obj2cad_acis::*;
use obj2cad_core::convert::{layer_color, Layer, Region, SurfaceEntity};
use obj2cad_core::CadModel;
use prim::Prim;

/// Vertices a seed's neighborhood needs before fitting (it grows ring by ring until it
/// has them, and stays small so it doesn't reach into the next surface).
const SEED_VERTICES: usize = 16;
/// Faces a seed's neighborhood may collect at most.
const SEED_FACES: usize = 40;
/// Neighboring faces whose normals differ by more than this (cosine) are across an edge.
const SMOOTH: f64 = 0.5;
/// A face joins a region only if its normal agrees with the surface's (cosine).
const NORMAL_AGREES: f64 = 0.64;
/// Smallest region worth a surface.
const MIN_FACES: usize = 4;
/// A tessellation's flat faces stay close to the surface they approximate: the chord
/// height (a face's farthest point from the surface) stays below this share of its
/// longest edge. It rules out surfaces that merely pass through the vertices.
const CHORD: f64 = 0.2;

/// A recognized region with its surface.
pub struct Found {
    pub region: Region,
    pub body: Body,
    /// The mesh entity most of its faces are in (for layer and color).
    pub entity: u32,
}

struct Tolerance {
    /// Per welded vertex.
    tol: Vec<f64>,
}

/// Find every surface in the model's meshes.
pub fn recognize(model: &CadModel) -> Vec<Found> {
    let mesh = Mesh::build(model);
    if mesh.faces() == 0 || mesh.size.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Vec::new();
    }
    // Never looser than a millionth of the model; never tighter than double precision.
    let coord_max = mesh
        .pos
        .iter()
        .flat_map(|p| p.iter())
        .fold(0.0f64, |m, &c| m.max(c.abs()));
    let floor = 1e-12 * (coord_max + mesh.size);
    let cap = (1e-6 * mesh.size).max(floor);
    let tols = Tolerance {
        tol: mesh.tol.iter().map(|&t| t.min(cap).max(floor)).collect(),
    };
    let mut assigned = vec![false; mesh.faces()];
    let mut tried = vec![false; mesh.faces()];
    let mut found: Vec<(Vec<u32>, Prim, Found)> = Vec::new();
    for seed in 0..mesh.faces() {
        if assigned[seed] || tried[seed] || mesh.area[seed] == 0.0 {
            continue;
        }
        tried[seed] = true;
        // The ring around the seed, or, where that reaches into another surface (a
        // band one face tall between two others), the seed with some of its neighbors.
        let ring = neighborhood(&mesh, seed, &assigned);
        let Some((hood, prim)) = std::iter::once(ring.clone())
            .chain(subsets(&mesh, seed, &ring))
            .find_map(|h| seed_fit(&mesh, &tols, &h).map(|p| (h, p)))
        else {
            continue;
        };
        let (faces, prim) = grow(&mesh, &tols, &hood, prim, &assigned);
        if let Some(f) = accept(&mesh, &tols, &faces, &prim) {
            for &x in &faces {
                assigned[x as usize] = true;
            }
            found.push((faces, prim, f));
        } else {
            // Faces that fit this surface won't seed a better one.
            for &x in &faces {
                tried[x as usize] = true;
            }
        }
    }
    merge(&mesh, &tols, &mut found);
    found.into_iter().map(|(_, _, f)| f).collect()
}

/// Join neighboring regions whose faces together lie on one of their surfaces (a thin
/// band of a fillet can pass for a sphere on its own, but belongs to the torus next to
/// it). The more general surface is tried first.
fn merge(mesh: &Mesh, tols: &Tolerance, found: &mut Vec<(Vec<u32>, Prim, Found)>) {
    loop {
        let mut owner = vec![usize::MAX; mesh.faces()];
        for (i, (faces, _, _)) in found.iter().enumerate() {
            for &f in faces {
                owner[f as usize] = i;
            }
        }
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for (i, (faces, _, _)) in found.iter().enumerate() {
            for &f in faces {
                for &g in mesh.neighbors(f as usize) {
                    let j = owner[g as usize];
                    if j != usize::MAX && j > i && !pairs.contains(&(i, j)) {
                        pairs.push((i, j));
                    }
                }
            }
        }
        let mut done = None;
        'pairs: for &(i, j) in &pairs {
            let mut union: Vec<u32> = found[i].0.iter().chain(&found[j].0).copied().collect();
            union.sort_unstable();
            let mut prims = [found[i].1, found[j].1];
            prims.sort_by_key(|p| std::cmp::Reverse(p.params()));
            for p in prims {
                let sign = orientation(mesh, &union, &p);
                let q = refit(mesh, tols, &union, p);
                if union
                    .iter()
                    .all(|&f| fits_face(mesh, tols, f as usize, &q, sign))
                {
                    if let Some(f) = accept(mesh, tols, &union, &q) {
                        done = Some((i, j, (union, q, f)));
                        break 'pairs;
                    }
                }
            }
        }
        match done {
            Some((i, j, merged)) => {
                found[i] = merged;
                found.remove(j);
            }
            None => return,
        }
    }
}

/// The drawing property (`obj2cad.curves`) for `count` surfaces written.
pub fn label(count: &str) -> String {
    format!("{count} surfaces, strict (every vertex within its stated precision)")
}

/// Add the recognized surfaces to `model`, on a layer of their own.
pub fn add_to(model: &mut CadModel) -> usize {
    let found = recognize(model);
    if found.is_empty() {
        return 0;
    }
    let taken: Vec<String> = model.layers.iter().map(|l| l.name.to_lowercase()).collect();
    let mut name = "Curves".to_owned();
    let mut n = 2;
    while taken.contains(&name.to_lowercase()) {
        name = format!("Curves~{n}");
        n += 1;
    }
    let layer = model.layers.len() as u32;
    model.layers.push(Layer {
        name,
        source: String::new(),
        color: layer_color(layer),
        file: None,
    });
    let count = found.len();
    for f in found {
        let color = model.meshes[f.entity as usize].color;
        model.surfaces.push(SurfaceEntity {
            layer,
            color,
            body: f.body,
            region: Some(f.region),
        });
    }
    count
}

fn region_vertices(mesh: &Mesh, faces: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = faces
        .iter()
        .flat_map(|&f| mesh.face(f as usize).iter().copied())
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Faces around `seed`, ring by ring, until they have `SEED_VERTICES` vertices; never
/// across sharp edges.
fn neighborhood(mesh: &Mesh, seed: usize, assigned: &[bool]) -> Vec<u32> {
    let mut out = vec![seed as u32];
    let mut ring_start = 0;
    while region_vertices(mesh, &out).len() < SEED_VERTICES && out.len() < SEED_FACES {
        let ring_end = out.len();
        for i in ring_start..ring_end {
            let f = out[i] as usize;
            for &g in mesh.neighbors(f) {
                let gu = g as usize;
                if assigned[gu] || out.contains(&g) || mesh.area[gu] == 0.0 {
                    continue;
                }
                if dot(mesh.normal[f], mesh.normal[gu]) >= SMOOTH {
                    out.push(g);
                }
            }
        }
        if out.len() == ring_end {
            break;
        }
        ring_start = ring_end;
    }
    out
}

/// The seed with each pair and triple of its (smooth) edge neighbors, largest first.
fn subsets(mesh: &Mesh, seed: usize, ring: &[u32]) -> Vec<Vec<u32>> {
    let near: Vec<u32> = mesh
        .neighbors(seed)
        .iter()
        .copied()
        .filter(|g| ring.contains(g))
        .collect();
    let mut out = Vec::new();
    if near.len() < 3 {
        return out;
    }
    for i in 0..near.len() {
        for j in i + 1..near.len() {
            for k in j + 1..near.len() {
                out.push(vec![seed as u32, near[i], near[j], near[k]]);
            }
        }
    }
    for i in 0..near.len() {
        for j in i + 1..near.len() {
            out.push(vec![seed as u32, near[i], near[j]]);
        }
    }
    out
}

/// The simplest surface all of the neighborhood's vertices lie on, if it is curved.
fn seed_fit(mesh: &Mesh, tols: &Tolerance, hood: &[u32]) -> Option<Prim> {
    let verts = region_vertices(mesh, hood);
    if verts.len() < 8 || hood.len() < 3 {
        return None;
    }
    let verts_needed = |p: &Prim| verts.len() >= p.params() + 3;
    let pts: Vec<V3> = verts.iter().map(|&v| mesh.pos[v as usize]).collect();
    let tol: Vec<f64> = verts.iter().map(|&v| tols.tol[v as usize]).collect();
    // Flat within tolerance: nothing to recognize.
    let (c, n, _) = prim::plane(&pts);
    if pts
        .iter()
        .zip(&tol)
        .all(|(&p, &t)| dot(sub(p, c), n).abs() <= t)
    {
        return None;
    }
    let normals: Vec<(V3, f64)> = hood
        .iter()
        .map(|&f| (mesh.normal[f as usize], mesh.area[f as usize]))
        .collect();
    let centers: Vec<V3> = hood.iter().map(|&f| mesh.center[f as usize]).collect();
    let vnormals = vertex_normals(mesh, hood, &verts);
    let edges: Vec<(usize, usize)> = {
        let at: std::collections::HashMap<u32, usize> =
            verts.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        let mut e: Vec<(usize, usize)> = hood
            .iter()
            .flat_map(|&f| {
                let idx = mesh.face(f as usize);
                (0..idx.len()).map(move |k| (idx[k], idx[(k + 1) % idx.len()]))
            })
            .map(|(a, b)| (at[&a.min(b)], at[&a.max(b)]))
            .collect();
        e.sort_unstable();
        e.dedup();
        e
    };
    let size = mesh.size;
    let candidates: [&dyn Fn() -> Option<Prim>; 4] = [
        &|| prim::sphere(&pts),
        &|| prim::cylinder(&pts, &normals),
        &|| prim::cone(&pts, &normals, &centers),
        &|| {
            if pts.len() >= 16 {
                prim::torus(&pts, &vnormals, &edges)
            } else {
                None
            }
        },
    ];
    // Every type that the vertices allow; the one the faces themselves hug wins (vertices
    // alone can't tell a cylinder from the sphere through its two end circles). Ties go
    // to the simpler type.
    let mut best: Option<(f64, Prim)> = None;
    for (ti, make) in candidates.iter().enumerate() {
        let Some(p0) = make() else { continue };
        // Real surfaces start close: sphere and cylinder estimates are near exact on data
        // that sits on them; cone and torus estimates within about 1e6 tolerances.
        // Refining a hopeless start only costs time.
        let limit = if ti >= 2 { 1e7 } else { 1e3 };
        if p0.worst(&pts, &tol).partial_cmp(&limit) == Some(std::cmp::Ordering::Greater)
            || p0.worst(&pts, &tol).is_nan()
        {
            continue;
        }
        let p = p0.refine(&pts, &tol, size);
        if !verts_needed(&p) || p.worst(&pts, &tol) > 1.0 || !curved_enough(&p, &pts, &tol) {
            continue;
        }
        let chord = hood
            .iter()
            .map(|&f| chord_height(mesh, f as usize, &p))
            .fold(0.0, f64::max);
        let fits = hood
            .iter()
            .all(|&f| chord_height(mesh, f as usize, &p) <= CHORD * longest_edge(mesh, f as usize));
        if fits && best.as_ref().is_none_or(|b| chord < b.0 * 0.99) {
            best = Some((chord, p));
        }
    }
    best.map(|b| b.1)
}

/// A face's farthest point (centroid or edge midpoint) from the surface.
fn chord_height(mesh: &Mesh, f: usize, p: &Prim) -> f64 {
    let idx = mesh.face(f);
    let mid = (0..idx.len()).map(|k| {
        let (a, b) = (
            mesh.pos[idx[k] as usize],
            mesh.pos[idx[(k + 1) % idx.len()] as usize],
        );
        p.distance(scale(add(a, b), 0.5)).abs()
    });
    mid.fold(p.distance(mesh.center[f]).abs(), f64::max)
}

fn longest_edge(mesh: &Mesh, f: usize) -> f64 {
    let idx = mesh.face(f);
    (0..idx.len())
        .map(|k| {
            dist(
                mesh.pos[idx[k] as usize],
                mesh.pos[idx[(k + 1) % idx.len()] as usize],
            )
        })
        .fold(0.0, f64::max)
}

fn extent(pts: &[V3]) -> f64 {
    let lo = pts.iter().fold([f64::INFINITY; 3], |m, p| {
        [m[0].min(p[0]), m[1].min(p[1]), m[2].min(p[2])]
    });
    let hi = pts.iter().fold([f64::NEG_INFINITY; 3], |m, p| {
        [m[0].max(p[0]), m[1].max(p[1]), m[2].max(p[2])]
    });
    dist(lo, hi)
}

/// Clearly not flat: the points leave their best plane by 20 tolerances or more, and the
/// surface isn't one that could pass for a plane over this patch.
fn curved_enough(p: &Prim, pts: &[V3], tol: &[f64]) -> bool {
    let (_, _, worst) = prim::plane(pts);
    let t = tol.iter().fold(0.0f64, |m, &x| m.max(x));
    let span = extent(pts);
    let radius_ok = match *p {
        Prim::Sphere { r, .. } | Prim::Cylinder { r, .. } => r < 1e3 * span,
        Prim::Torus { minor, major, .. } => minor < 1e3 * span && major < 1e4 * span,
        Prim::Cone { .. } => true,
    };
    worst >= 20.0 * t && radius_ok
}

fn vertex_normals(mesh: &Mesh, faces: &[u32], verts: &[u32]) -> Vec<V3> {
    let mut acc: std::collections::HashMap<u32, V3> =
        verts.iter().map(|&v| (v, [0.0; 3])).collect();
    for &f in faces {
        let n = scale(mesh.normal[f as usize], mesh.area[f as usize]);
        for &v in mesh.face(f as usize) {
            if let Some(a) = acc.get_mut(&v) {
                *a = add(*a, n);
            }
        }
    }
    verts.iter().map(|v| unit(acc[v])).collect()
}

/// Which way the mesh's faces point relative to the surface's outward normal (+1/-1).
fn orientation(mesh: &Mesh, faces: &[u32], p: &Prim) -> f64 {
    let s: f64 = faces
        .iter()
        .map(|&f| {
            dot(mesh.normal[f as usize], p.normal(mesh.center[f as usize])) * mesh.area[f as usize]
        })
        .sum();
    if s < 0.0 {
        -1.0
    } else {
        1.0
    }
}

fn fits_face(mesh: &Mesh, tols: &Tolerance, f: usize, p: &Prim, sign: f64) -> bool {
    mesh.area[f] > 0.0
        && mesh
            .face(f)
            .iter()
            .all(|&v| (p.distance(mesh.pos[v as usize]) / tols.tol[v as usize]).abs() <= 1.0)
        && dot(mesh.normal[f], p.normal(mesh.center[f])) * sign >= NORMAL_AGREES
        && chord_height(mesh, f, p) <= CHORD * longest_edge(mesh, f)
}

/// Within 50 tolerances and facing the right way: worth a refit.
fn near_face(mesh: &Mesh, tols: &Tolerance, f: usize, p: &Prim, sign: f64) -> bool {
    mesh.area[f] > 0.0
        && mesh
            .face(f)
            .iter()
            .all(|&v| (p.distance(mesh.pos[v as usize]) / tols.tol[v as usize]).abs() <= 50.0)
        && dot(mesh.normal[f], p.normal(mesh.center[f])) * sign >= NORMAL_AGREES
}

/// Grow from the neighborhood's faces that fit, refitting as the region grows.
fn grow(
    mesh: &Mesh,
    tols: &Tolerance,
    hood: &[u32],
    mut p: Prim,
    assigned: &[bool],
) -> (Vec<u32>, Prim) {
    let sign = orientation(mesh, hood, &p);
    let mut member = vec![false; mesh.faces()];
    let mut faces: Vec<u32> = Vec::new();
    for &f in hood {
        if fits_face(mesh, tols, f as usize, &p, sign) {
            member[f as usize] = true;
            faces.push(f);
        }
    }
    let mut last_fit = faces.len();
    for _round in 0..4 {
        let before = faces.len();
        let mut queue: std::collections::VecDeque<u32> = faces.iter().copied().collect();
        while let Some(f) = queue.pop_front() {
            for &g in mesh.neighbors(f as usize) {
                let gu = g as usize;
                if member[gu] || assigned[gu] {
                    continue;
                }
                if !fits_face(mesh, tols, gu, &p, sign) {
                    // Nearly on it: the surface fitted to a smaller patch may just need
                    // refining. Keep the refit only if every vertex still fits.
                    if !near_face(mesh, tols, gu, &p, sign) {
                        continue;
                    }
                    let mut with = faces.clone();
                    with.push(g);
                    let q = refit(mesh, tols, &with, p);
                    if q == p
                        || !fits_face(mesh, tols, gu, &q, sign)
                        || !faces
                            .iter()
                            .all(|&h| fits_face(mesh, tols, h as usize, &q, sign))
                    {
                        continue;
                    }
                    p = q;
                }
                member[gu] = true;
                faces.push(g);
                queue.push_back(g);
                if faces.len() >= last_fit * 2 {
                    p = refit(mesh, tols, &faces, p);
                    last_fit = faces.len();
                }
            }
        }
        p = refit(mesh, tols, &faces, p);
        if faces.len() == before {
            break;
        }
    }
    faces.sort_unstable();
    (faces, p)
}

/// Refit to all of the region's vertices; keep the old surface if the new one is worse.
fn refit(mesh: &Mesh, tols: &Tolerance, faces: &[u32], p: Prim) -> Prim {
    let verts = region_vertices(mesh, faces);
    let pts: Vec<V3> = verts.iter().map(|&v| mesh.pos[v as usize]).collect();
    let tol: Vec<f64> = verts.iter().map(|&v| tols.tol[v as usize]).collect();
    let q = p.refine(&pts, &tol, mesh.size);
    if q.worst(&pts, &tol) <= p.worst(&pts, &tol).max(1.0) && q.worst(&pts, &tol) <= 1.0 {
        q
    } else {
        p
    }
}

fn accept(mesh: &Mesh, tols: &Tolerance, faces: &[u32], p: &Prim) -> Option<Found> {
    if faces.len() < MIN_FACES {
        return None;
    }
    let verts = region_vertices(mesh, faces);
    if verts.len() < 2 * p.params() + 2 {
        return None;
    }
    let pts: Vec<V3> = verts.iter().map(|&v| mesh.pos[v as usize]).collect();
    let tol: Vec<f64> = verts.iter().map(|&v| tols.tol[v as usize]).collect();
    if p.worst(&pts, &tol) > 1.0 || !curved_enough(p, &pts, &tol) {
        return None;
    }
    let tmax = tol.iter().fold(0.0f64, |m, &x| m.max(x));
    let reversed = orientation(mesh, faces, p) < 0.0;
    let body = boundary::body(mesh, faces, p, reversed, tmax)?;
    let max_deviation = pts.iter().map(|&x| p.distance(x).abs()).fold(0.0, f64::max);
    let mut sources: Vec<u32> = faces.iter().map(|&f| mesh.source[f as usize]).collect();
    sources.sort_unstable();
    let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for &f in faces {
        *counts.entry(mesh.entity[f as usize]).or_default() += 1;
    }
    let entity = counts
        .iter()
        .max_by_key(|(e, n)| (**n, std::cmp::Reverse(**e)))
        .map_or(0, |(e, _)| *e);
    Some(Found {
        region: Region {
            kind: p.kind(),
            faces: sources,
            max_deviation,
            tolerance: tmax,
        },
        body,
        entity,
    })
}
