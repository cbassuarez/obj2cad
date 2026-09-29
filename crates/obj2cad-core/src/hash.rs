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
    sha256_hex(&parity_stream(model))
}

/// The exact bytes `parity_hash` digests. Exposed so the web app can hash it with the
/// browser's hardware-accelerated SHA-256; the result is identical.
pub fn parity_stream(model: &CadModel) -> Vec<u8> {
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
        fn append_to(mut self, out: &mut Vec<u8>, tag: u8) {
            let bytes = &self.bytes;
            let rec = |&(s, l): &(u32, u32)| &bytes[s as usize..(s + l) as usize];
            self.spans.sort_unstable_by(|a, b| rec(a).cmp(rec(b)));
            out.push(tag);
            out.extend_from_slice(&(self.spans.len() as u64).to_le_bytes());
            for span in &self.spans {
                out.extend_from_slice(&span.1.to_le_bytes());
                out.extend_from_slice(rec(span));
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

    let mut out = Vec::with_capacity(
        32 + refs * 24 + n_faces * 4 + lines.bytes.len() + points.bytes.len() * 2,
    );
    out.extend_from_slice(b"obj2cad-parity-v1\0");
    faces.append_to(&mut out, b'f');
    lines.append_to(&mut out, b'l');
    points.append_to(&mut out, b'p');
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
        splines.append_to(&mut out, b's');
    }
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
