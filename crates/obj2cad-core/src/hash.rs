//! Canonical parity hash (`obj2cad-parity-v1`).
//!
//! Defined on geometry as positions, not indices, so it is independent of how a writer
//! lays out vertices across entities:
//!
//! * each face / polyline is a record of its vertices' IEEE-754 bits (x, y, z, big-endian,
//!   in element order, so winding and start vertex matter); each point is a one-vertex record;
//! * records of each kind are sorted bytewise, then hashed as
//!   `tag, count:u64le, (len:u32le, bytes)*` under a version prefix;
//! * free-form curves (splines), when a file has any, follow as a fourth kind `s`: degree
//!   and knot count (u32 BE), then the bits of the knots, control points and weights.
//!
//! The Python harness (`tests/harness/parity.py`) implements the same definition
//! independently and computes it from the source OBJ and from the read-back CAD file.

use crate::convert::CadModel;
use sha2::{Digest, Sha256};

pub fn parity_hash(model: &CadModel) -> String {
    let mut h = Sha256::new();
    write_parity(model, &mut h);
    hex(&h.finalize())
}

/// The exact bytes `parity_hash` digests (for tests and tools; hashing streams them).
pub fn parity_stream(model: &CadModel) -> Vec<u8> {
    let mut out = Vec::new();
    write_parity(model, &mut out);
    out
}

/// Where the parity bytes go: a hasher, or a buffer.
trait Out {
    fn put(&mut self, bytes: &[u8]);
}

impl Out for Sha256 {
    fn put(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
}

impl Out for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

fn write_parity(model: &CadModel, out: &mut impl Out) {
    // All records of one kind live in a single flat buffer; sorting offsets instead of
    // owned vectors avoids one allocation per face.
    struct Records {
        bytes: Vec<u8>,
        spans: Vec<(u32, u32)>,
    }
    impl Records {
        fn with_capacity(n: usize, bytes: usize) -> Self {
            Self {
                bytes: Vec::with_capacity(bytes),
                spans: Vec::with_capacity(n),
            }
        }
        fn push(&mut self, model: &CadModel, verts: impl Iterator<Item = u32>) {
            let start = self.bytes.len();
            for v in verts {
                for c in model.position(v) {
                    self.bytes.extend_from_slice(&c.to_bits().to_be_bytes());
                }
            }
            self.spans
                .push((start as u32, (self.bytes.len() - start) as u32));
        }
        fn write(self, out: &mut impl Out, tag: u8) {
            let bytes = &self.bytes;
            let rec = |s: u32, l: u32| &bytes[s as usize..(s + l) as usize];
            // Bytewise order, compared first as a big-endian integer of each record's
            // first 16 bytes (zero-padded), which orders the same way and is far cheaper.
            let prefix = |s: u32, l: u32| {
                let mut k = [0u8; 16];
                let r = rec(s, l);
                let n = r.len().min(16);
                k[..n].copy_from_slice(&r[..n]);
                u128::from_be_bytes(k)
            };
            let mut keyed: Vec<(u128, u32, u32)> = self
                .spans
                .iter()
                .map(|&(s, l)| (prefix(s, l), s, l))
                .collect();
            keyed.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0).then_with(|| rec(a.1, a.2).cmp(rec(b.1, b.2)))
            });
            out.put(&[tag]);
            out.put(&(keyed.len() as u64).to_le_bytes());
            for &(_, s, l) in &keyed {
                out.put(&l.to_le_bytes());
                out.put(rec(s, l));
            }
        }
    }

    let refs: usize = model.meshes.iter().map(|m| m.face_indices.len()).sum();
    let n_faces: usize = model.meshes.iter().map(|m| m.face_count()).sum();
    let mut faces = Records::with_capacity(n_faces, refs * 24);
    for m in &model.meshes {
        for f in m.faces() {
            faces.push(model, f.iter().map(|&i| m.vertices[i as usize]));
        }
    }
    let mut lines = Records::with_capacity(model.polylines.len(), 0);
    for l in &model.polylines {
        lines.push(model, l.vertices.iter().copied());
    }
    let mut points = Records::with_capacity(model.points.len(), model.points.len() * 24);
    for p in &model.points {
        points.push(model, std::iter::once(p.vertex));
    }

    out.put(b"obj2cad-parity-v1\0");
    faces.write(out, b'f');
    lines.write(out, b'l');
    points.write(out, b'p');
    // Splines (free-form curves), only when there are any, so every other file keeps its
    // hash: degree (u32 BE), knot count (u32 BE), knots, control points, weights (bits).
    if !model.splines.is_empty() {
        let mut splines = Records::with_capacity(model.splines.len(), 0);
        for c in &model.splines {
            let start = splines.bytes.len();
            splines.bytes.extend_from_slice(&c.degree.to_be_bytes());
            splines
                .bytes
                .extend_from_slice(&(c.knots.len() as u32).to_be_bytes());
            for k in &c.knots {
                splines.bytes.extend_from_slice(&k.to_bits().to_be_bytes());
            }
            for &v in &c.control {
                for x in model.position(v) {
                    splines.bytes.extend_from_slice(&x.to_bits().to_be_bytes());
                }
            }
            for w in c.weights.iter().flatten() {
                splines.bytes.extend_from_slice(&w.to_bits().to_be_bytes());
            }
            splines
                .spans
                .push((start as u32, (splines.bytes.len() - start) as u32));
        }
        splines.write(out, b's');
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
