//! Small bodies with known answers, for checking ACIS output in AutoCAD before trusting
//! recognized surfaces (docs/ACCEPTANCE.md) and for tests.

use crate::builder::{Builder, Seg};
use crate::*;

const Z: V3 = [0.0, 0.0, 1.0];
const X: V3 = [1.0, 0.0, 0.0];

fn line(from: V3, to: V3) -> Seg {
    Seg::Line { from, to }
}

fn arc(center: V3, normal: V3, from: V3, to: V3) -> Seg {
    Seg::Arc {
        center,
        normal,
        from,
        to,
    }
}

fn cylinder_surface(radius: f64) -> Surface {
    Surface::Cone {
        center: [0.0; 3],
        axis: Z,
        u_dir: X,
        radius,
        slope: 0.0,
    }
}

/// A flat 40 × 20 rectangle (a sheet).
pub fn plane_sheet() -> Body {
    let p = |x: f64, y: f64| [x, y, 0.0];
    let mut b = Builder::new(false);
    b.face(
        Surface::Plane {
            root: [0.0; 3],
            normal: Z,
            u_dir: X,
        },
        false,
        &[vec![
            line(p(0.0, 0.0), p(40.0, 0.0)),
            line(p(40.0, 0.0), p(40.0, 20.0)),
            line(p(40.0, 20.0), p(0.0, 20.0)),
            line(p(0.0, 20.0), p(0.0, 0.0)),
        ]],
    );
    b.finish()
}

/// A closed 30 × 20 × 10 box (a solid).
pub fn box_solid() -> Body {
    let (a, bb, c) = (30.0, 20.0, 10.0);
    let v = |i: usize| -> V3 {
        [
            if i & 1 != 0 { a } else { 0.0 },
            if i & 2 != 0 { bb } else { 0.0 },
            if i & 4 != 0 { c } else { 0.0 },
        ]
    };
    // Faces as corner rings, counterclockwise seen from outside.
    let faces: [([usize; 4], V3); 6] = [
        ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
    ];
    let mut b = Builder::new(true);
    for (ring, normal) in faces {
        let segs: Vec<Seg> = (0..4)
            .map(|k| line(v(ring[k]), v(ring[(k + 1) % 4])))
            .collect();
        let root = v(ring[0]);
        let u_dir = unit(sub(v(ring[1]), root));
        b.face(
            Surface::Plane {
                root,
                normal,
                u_dir,
            },
            false,
            &[segs],
        );
    }
    b.finish()
}

/// A closed cylinder, radius 10, height 25 (a solid: two caps and the side).
pub fn cylinder_solid() -> Body {
    let (r, h) = (10.0, 25.0);
    let (c0, c1) = ([0.0, 0.0, 0.0], [0.0, 0.0, h]);
    let (p0, p1) = ([r, 0.0, 0.0], [r, 0.0, h]);
    let mut b = Builder::new(true);
    b.face(
        Surface::Plane {
            root: c0,
            normal: [0.0, 0.0, -1.0],
            u_dir: X,
        },
        false,
        &[vec![arc(c0, [0.0, 0.0, -1.0], p0, p0)]],
    );
    b.face(
        Surface::Plane {
            root: c1,
            normal: Z,
            u_dir: X,
        },
        false,
        &[vec![arc(c1, Z, p1, p1)]],
    );
    b.face(
        cylinder_surface(r),
        false,
        &[
            vec![arc(c0, Z, p0, p0)],
            vec![arc(c1, [0.0, 0.0, -1.0], p1, p1)],
        ],
    );
    b.finish()
}

/// The side of a cylinder alone (a sheet, like a recognized region).
pub fn cylinder_band() -> Body {
    let (r, h) = (10.0, 25.0);
    let (p0, p1) = ([r, 0.0, 0.0], [r, 0.0, h]);
    let mut b = Builder::new(false);
    b.face(
        cylinder_surface(r),
        false,
        &[
            vec![arc([0.0; 3], Z, p0, p0)],
            vec![arc([0.0, 0.0, h], [0.0, 0.0, -1.0], p1, p1)],
        ],
    );
    b.finish()
}

/// Half a cylinder's side: two straight edges and two arcs.
pub fn half_cylinder() -> Body {
    let (r, h) = (10.0, 25.0);
    let (a0, b0, a1, b1) = ([r, 0.0, 0.0], [-r, 0.0, 0.0], [r, 0.0, h], [-r, 0.0, h]);
    let mut b = Builder::new(false);
    b.face(
        cylinder_surface(r),
        false,
        &[vec![
            arc([0.0; 3], Z, a0, b0),
            line(b0, b1),
            arc([0.0, 0.0, h], [0.0, 0.0, -1.0], b1, a1),
            line(a1, a0),
        ]],
    );
    b.finish()
}

/// The side of a cone frustum: radius 12 at the bottom, 6 at height 18.
pub fn cone_band() -> Body {
    let (r0, r1, h) = (12.0, 6.0, 18.0);
    let mut b = Builder::new(false);
    let (p0, p1) = ([r0, 0.0, 0.0], [r1, 0.0, h]);
    b.face(
        Surface::Cone {
            center: [0.0; 3],
            axis: Z,
            u_dir: X,
            radius: r0,
            slope: (r1 - r0) / h,
        },
        false,
        &[
            vec![arc([0.0; 3], Z, p0, p0)],
            vec![arc([0.0, 0.0, h], [0.0, 0.0, -1.0], p1, p1)],
        ],
    );
    b.finish()
}

/// A full sphere, radius 8 (a solid with one face and no edges).
pub fn sphere_solid() -> Body {
    let mut b = Builder::new(true);
    b.face(
        Surface::Sphere {
            center: [0.0; 3],
            radius: 8.0,
            u_dir: X,
            pole: Z,
        },
        false,
        &[],
    );
    b.finish()
}

/// The top of a sphere above z = 4 (radius 8): one circular edge.
pub fn sphere_cap() -> Body {
    let (r, z0): (f64, f64) = (8.0, 4.0);
    let rho = (r * r - z0 * z0).sqrt();
    let p = [rho, 0.0, z0];
    let mut b = Builder::new(false);
    b.face(
        Surface::Sphere {
            center: [0.0; 3],
            radius: r,
            u_dir: X,
            pole: Z,
        },
        false,
        &[vec![arc([0.0, 0.0, z0], Z, p, p)]],
    );
    b.finish()
}

/// A full torus: 15 to the tube's center, tube radius 4.
pub fn torus_solid() -> Body {
    let mut b = Builder::new(true);
    b.face(
        Surface::Torus {
            center: [0.0; 3],
            axis: Z,
            major: 15.0,
            minor: 4.0,
            u_dir: X,
        },
        false,
        &[],
    );
    b.finish()
}

/// Every sample, by file name.
pub fn all() -> Vec<(&'static str, Body)> {
    vec![
        ("acis_plane_sheet", plane_sheet()),
        ("acis_box", box_solid()),
        ("acis_cylinder", cylinder_solid()),
        ("acis_cylinder_band", cylinder_band()),
        ("acis_half_cylinder", half_cylinder()),
        ("acis_cone_band", cone_band()),
        ("acis_sphere", sphere_solid()),
        ("acis_sphere_cap", sphere_cap()),
        ("acis_torus", torus_solid()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_consistent() {
        for (name, body) in all() {
            body.validate(1e-9)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn box_edges_are_shared() {
        let b = box_solid();
        assert_eq!((b.vertices.len(), b.edges.len(), b.faces.len()), (8, 12, 6));
        let uses: usize = b
            .faces
            .iter()
            .flat_map(|f| &f.loops)
            .map(|l| l.coedges.len())
            .sum();
        assert_eq!(uses, 24);
    }

    #[test]
    fn sat_text_is_well_formed() {
        let t = crate::sat::write(&cylinder_solid(), "obj2cad", 0.0);
        assert!(t.starts_with("700 0 1 0\n"));
        assert!(t.contains("cone-surface $-1 -1 $-1 0.0 0.0 0.0 0.0 0.0 1.0 10.0 0.0 0.0 1.0 I I 0.0 1.0 10.0 forward I I I I #"));
        assert!(t.ends_with("End-of-ACIS-data\n"));
        let lines = t.lines().filter(|l| l.ends_with('#')).count();
        // body lump shell, 3 faces, 4 loops, 4 coedges, 2 edges, 2 vertices, 2 points,
        // 3 surfaces, 2 curves
        assert_eq!(lines, 3 + 3 + 4 + 4 + 2 + 2 + 2 + 3 + 2);
    }
}
