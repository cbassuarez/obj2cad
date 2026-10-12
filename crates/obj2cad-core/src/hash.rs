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
            // Bytewise order, compared first as two big-endian integers of each record's
            // first 32 bytes (zero-padded), which order the same way and are far cheaper;
            // that decides every point (24 bytes) and most faces without touching bytes.
            let key = |s: u32, l: u32| {
                let mut k = [0u8; 32];
                let r = rec(s, l);
                let n = r.len().min(32);
                k[..n].copy_from_slice(&r[..n]);
                let (a, b) = k.split_at(16);
                (
                    u128::from_be_bytes(a.try_into().expect("16 bytes")),
                    u128::from_be_bytes(b.try_into().expect("16 bytes")),
                )
            };
            let mut keyed: Vec<((u128, u128), u32, u32)> =
                self.spans.iter().map(|&(s, l)| (key(s, l), s, l)).collect();
            keyed.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0).then_with(|| {
                    if a.2.max(b.2) <= 32 {
                        a.2.cmp(&b.2) // equal zero-padded prefixes: the shorter one first
                    } else {
                        rec(a.1, a.2).cmp(rec(b.1, b.2))
                    }
                })
            });
            // Out in large pieces (a hasher takes them far faster than one per record).
            let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
            buf.push(tag);
            buf.extend_from_slice(&(keyed.len() as u64).to_le_bytes());
            for &(_, s, l) in &keyed {
                buf.extend_from_slice(&l.to_le_bytes());
                buf.extend_from_slice(rec(s, l));
                if buf.len() >= (1 << 16) - 4096 {
                    out.put(&buf);
                    buf.clear();
                }
            }
            out.put(&buf);
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

    out.put(b"obj2cad-parity-v1\0");
    faces.write(out, b'f');
    lines.write(out, b'l');
    write_points(model, out, POINT_BATCH);
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

/// Points sorted at a time, at most (about 190 MB of keys).
const POINT_BATCH: usize = 8 << 20;

/// The points' records (`p`), sorted, in batches of about `batch` so that a point cloud
/// of any size takes bounded memory: splitters taken from a sorted sample cut the keys
/// into ranges, and each range is gathered, sorted and written in turn. A point's record
/// is its 24 bytes, so bytewise order is the order of its (x, y, z) bits.
fn write_points(model: &CadModel, out: &mut impl Out, batch: usize) {
    let key = |p: &crate::convert::PointEntity| model.position(p.vertex).map(f64::to_bits);
    let n = model.points.len();
    let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
    buf.push(b'p');
    buf.extend_from_slice(&(n as u64).to_le_bytes());

    let ranges = n.div_ceil(batch.max(1)).max(1);
    // Range r is splitters[r - 1]..splitters[r], open at both ends.
    let splitters: Vec<[u64; 3]> = if ranges == 1 {
        Vec::new()
    } else {
        let every = (n / (64 * ranges)).max(1);
        let mut sample: Vec<[u64; 3]> = model.points.iter().step_by(every).map(key).collect();
        sample.sort_unstable();
        (1..ranges)
            .map(|r| sample[r * sample.len() / ranges])
            .collect()
    };
    let mut keys: Vec<[u64; 3]> = Vec::new();
    for r in 0..ranges {
        let lo = r.checked_sub(1).map(|i| splitters[i]);
        let hi = splitters.get(r).copied();
        if lo.is_some() && lo == hi {
            continue;
        }
        keys.clear();
        keys.extend(
            model
                .points
                .iter()
                .map(key)
                .filter(|k| lo.is_none_or(|lo| *k >= lo) && hi.is_none_or(|hi| *k < hi)),
        );
        keys.sort_unstable();
        for k in &keys {
            buf.extend_from_slice(&24u32.to_le_bytes());
            for c in k {
                buf.extend_from_slice(&c.to_be_bytes());
            }
            if buf.len() >= (1 << 16) - 4096 {
                out.put(&buf);
                buf.clear();
            }
        }
    }
    out.put(&buf);
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The length and SHA-256 of everything `r` reads (a file too large to hold).
pub fn sha256_read(mut r: impl std::io::Read) -> std::io::Result<(u64, String)> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut len = 0u64;
    loop {
        match r.read(&mut buf) {
            Ok(0) => return Ok((len, hex(&h.finalize()))),
            Ok(n) => {
                h.update(&buf[..n]);
                len += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{convert, Options};

    #[test]
    fn points_sorted_in_batches_hash_as_sorted_at_once() {
        // Repeated points and points that differ only in y or z, in no order.
        let src: String = (0..5000u32)
            .map(|i| {
                let h = i.wrapping_mul(2_654_435_761);
                format!("{} {} {}\n", h % 7, (h >> 8) % 5, i32::from(h & 1 == 0) - 1)
            })
            .collect();
        let doc = crate::xyz::parse(src.as_bytes(), "scan.xyz").unwrap();
        let model = convert(&doc, None, Options::default());
        let mut whole = Vec::new();
        write_points(&model, &mut whole, usize::MAX);
        for batch in [1, 7, 100, 4999] {
            let mut batched = Vec::new();
            write_points(&model, &mut batched, batch);
            assert!(batched == whole, "batches of {batch}");
        }
        // The records are sorted bytewise.
        let recs: Vec<&[u8]> = whole[9..].chunks(28).map(|r| &r[4..]).collect();
        assert_eq!(recs.len(), 5000);
        assert!(recs.windows(2).all(|w| w[0] <= w[1]));
    }
}
