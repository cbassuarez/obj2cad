//! The model's faces as one welded mesh, with each vertex's precision.

use obj2cad_acis::*;
use obj2cad_core::CadModel;
use std::collections::HashMap;

pub struct Mesh {
    /// Welded positions (in drawing coordinates).
    pub pos: Vec<V3>,
    /// How far each vertex may be from a surface: half its stated precision (see
    /// [`quantum`]), across the three coordinates.
    pub tol: Vec<f64>,
    pub face_off: Vec<u32>,
    pub face_idx: Vec<u32>,
    /// Source face (index into `ObjDocument::faces`) and mesh entity of each face.
    pub source: Vec<u32>,
    pub entity: Vec<u32>,
    pub normal: Vec<V3>,
    pub area: Vec<f64>,
    pub center: Vec<V3>,
    /// Faces sharing an edge with each face.
    pub nbr_off: Vec<u32>,
    pub nbr: Vec<u32>,
    /// Undirected edge → how many faces use it (for boundaries).
    pub edge_uses: HashMap<(u32, u32), u32>,
    /// The model's size (bounding box diagonal).
    pub size: f64,
}

/// The precision a number's text states: `1.234560` → 1e-6, `1.5e-3` → 1e-4, `12` → 1.
pub fn quantum(text: &str) -> f64 {
    let t = text.trim_start_matches(['+', '-']);
    let (mant, exp) = match t.find(['e', 'E']) {
        Some(i) => (&t[..i], t[i + 1..].parse::<i32>().unwrap_or(0)),
        None => (t, 0),
    };
    let decimals = mant.split_once('.').map_or(0, |(_, f)| f.len() as i32);
    10f64.powi(exp - decimals)
}

impl Mesh {
    pub fn face(&self, f: usize) -> &[u32] {
        &self.face_idx[self.face_off[f] as usize..self.face_off[f + 1] as usize]
    }

    pub fn neighbors(&self, f: usize) -> &[u32] {
        &self.nbr[self.nbr_off[f] as usize..self.nbr_off[f + 1] as usize]
    }

    pub fn faces(&self) -> usize {
        self.normal.len()
    }

    pub fn build(model: &CadModel) -> Mesh {
        let mut weld: HashMap<[u64; 3], u32> = HashMap::new();
        let mut pos: Vec<V3> = Vec::new();
        let mut tol: Vec<f64> = Vec::new();
        let (mut face_off, mut face_idx, mut source, mut entity) =
            (vec![0u32], Vec::new(), Vec::new(), Vec::new());
        for (ei, m) in model.meshes.iter().enumerate() {
            for (k, f) in m.faces().enumerate() {
                for &li in f {
                    let v = m.vertices[li as usize];
                    let p = model.position(v);
                    let t = 0.5
                        * (0..3)
                            .map(|a| quantum(&model.coord_text(v, a)).powi(2))
                            .sum::<f64>()
                            .sqrt();
                    let id = *weld.entry(p.map(f64::to_bits)).or_insert_with(|| {
                        pos.push(p);
                        tol.push(t);
                        (pos.len() - 1) as u32
                    });
                    // Several texts for one position: the most precise one counts.
                    tol[id as usize] = tol[id as usize].min(t);
                    face_idx.push(id);
                }
                face_off.push(face_idx.len() as u32);
                source.push(m.source_faces[k]);
                entity.push(ei as u32);
            }
        }
        let n = face_off.len() - 1;
        let (mut normal, mut area, mut center) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let mut edges: Vec<(u32, u32, u32)> = Vec::new();
        for f in 0..n {
            let idx = &face_idx[face_off[f] as usize..face_off[f + 1] as usize];
            let mut nn = [0.0; 3];
            let mut c = [0.0; 3];
            for (k, &i) in idx.iter().enumerate() {
                let (a, b) = (pos[i as usize], pos[idx[(k + 1) % idx.len()] as usize]);
                nn = add(nn, cross(a, b));
                c = add(c, a);
                let (lo, hi) = (
                    i.min(idx[(k + 1) % idx.len()]),
                    i.max(idx[(k + 1) % idx.len()]),
                );
                if lo != hi {
                    edges.push((lo, hi, f as u32));
                }
            }
            let len = norm(nn);
            area.push(len / 2.0);
            normal.push(if len > 0.0 {
                scale(nn, 1.0 / len)
            } else {
                [0.0; 3]
            });
            center.push(scale(c, 1.0 / idx.len() as f64));
        }
        edges.sort_unstable();
        edges.dedup();
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut edge_uses = HashMap::new();
        let mut i = 0;
        while i < edges.len() {
            let mut j = i;
            while j < edges.len() && edges[j].0 == edges[i].0 && edges[j].1 == edges[i].1 {
                j += 1;
            }
            edge_uses.insert((edges[i].0, edges[i].1), (j - i) as u32);
            for a in i..j {
                for b in i..j {
                    if a != b {
                        lists[edges[a].2 as usize].push(edges[b].2);
                    }
                }
            }
            i = j;
        }
        let mut nbr_off = vec![0u32];
        let mut nbr = Vec::new();
        for mut l in lists {
            l.sort_unstable();
            l.dedup();
            nbr.extend(l);
            nbr_off.push(nbr.len() as u32);
        }
        let size = if pos.is_empty() {
            0.0
        } else {
            let lo = pos.iter().fold([f64::INFINITY; 3], |m, p| {
                [m[0].min(p[0]), m[1].min(p[1]), m[2].min(p[2])]
            });
            let hi = pos.iter().fold([f64::NEG_INFINITY; 3], |m, p| {
                [m[0].max(p[0]), m[1].max(p[1]), m[2].max(p[2])]
            });
            dist(lo, hi)
        };
        Mesh {
            pos,
            tol,
            face_off,
            face_idx,
            source,
            entity,
            normal,
            area,
            center,
            nbr_off,
            nbr,
            edge_uses,
            size,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quantum_reads_the_stated_precision() {
        assert_eq!(super::quantum("1.234560"), 1e-6);
        assert_eq!(super::quantum("-0.000000"), 1e-6);
        assert!((super::quantum("1.5e-3") - 1e-4).abs() < 1e-20);
        assert_eq!(super::quantum("12"), 1.0);
        assert_eq!(super::quantum("1e3"), 1000.0);
    }
}
