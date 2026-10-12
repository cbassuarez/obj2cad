//! obj2cad: objects written by `io::dwg::streaming`, one at a time, never all in memory.

use super::common::{write_modular_char_bytes, write_modular_short_bytes};
use super::DwgObjectWriter;
use crate::entities::Point;
use crate::error::Result;
use crate::io::dwg::crc;
use crate::io::dwg::dwg_reference_type::DwgReferenceType;

/// Bytes a hard-ownership reference to `handle` takes in a handle stream: a code byte
/// (type and length) and the handle's significant bytes.
pub(crate) fn reference_len(handle: u64) -> u64 {
    1 + u64::from((64 - handle.leading_zeros()).div_ceil(8))
}

/// Bytes the references to `count` handles from `first` take (see [`reference_len`]),
/// counted a byte length at a time rather than one handle at a time.
pub(crate) fn references_len(first: u64, count: u64) -> u64 {
    let end = first + count;
    let mut total = 0;
    let mut at = first;
    while at < end {
        let bytes = reference_len(at);
        // The first handle that needs one more byte.
        let next = if bytes > 8 { end } else { 1u64 << (8 * (bytes - 1)) };
        let upto = next.min(end);
        total += (upto - at) * bytes;
        at = upto;
    }
    total
}

impl DwgObjectWriter<'_> {
    /// A POINT's object record (size, data, CRC), as `write_point` frames it, without
    /// keeping it: the bytes are valid until the next call.
    pub(crate) fn encode_point(&mut self, point: &Point) -> &[u8] {
        self.output.clear();
        self.streaming = true;
        self.write_point(point);
        self.streaming = false;
        &self.output
    }

    /// The deferred BLOCK_HEADER (see `deferred_header`), owning its document entities
    /// and then the `count` streamed entities whose handles start at `first`. Their
    /// references are spliced into the record's handle stream as it is written, so the
    /// record (five bytes per entity) is never in memory whole. Returns its length.
    pub(crate) fn write_deferred_header(
        &mut self,
        first: u64,
        count: u64,
        emit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u64> {
        let handle = self.deferred_header.expect("a deferred block header");
        let record = self
            .document
            .block_records
            .iter()
            .find(|r| r.handle == handle)
            .expect("the deferred block record is in the document");
        let entities = std::mem::take(&mut self.deferred_entities);
        let splice_at = self.write_block_header_body(record, &entities, count as usize);
        let data = self.writer.merge();
        let handle_start = self.writer.handle_start_bits() as u64;
        self.writer.reset();

        // The references go in at a whole number of bytes into the handle stream, which
        // starts `handle_start` bits into the record: everything after them keeps its
        // alignment, so the rest of the record follows unchanged.
        let inserted = references_len(first, count);
        let at_bit = handle_start + 8 * splice_at as u64;
        let (at, shift) = ((at_bit / 8) as usize, (at_bit % 8) as u32);
        let size = data.len() as u64 + inserted;
        let mut head = Vec::new();
        write_modular_short_bytes(&mut head, size as usize);
        if self.version.r2010_plus() {
            let handle_bits = size * 8 - handle_start;
            write_modular_char_bytes(&mut head, handle_bits as usize);
        }

        let mut crc = crc::crc16(crc::CRC16_SEED, &head);
        let mut written = 0u64;
        let mut out = |bytes: &[u8], crc: &mut u16| -> Result<()> {
            *crc = crc::crc16(*crc, bytes);
            written += bytes.len() as u64;
            emit(bytes)
        };
        // The head is in `crc` already.
        out(&head, &mut 0)?;
        out(&data[..at], &mut crc)?;

        // The byte the references start in: its first `shift` bits are the record's.
        let keep = if shift == 0 { 0 } else { data[at] & !(0xFFu8 >> shift) };
        let mut carry = keep;
        let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
        let push = |b: u8, carry: &mut u8, buf: &mut Vec<u8>| {
            if shift == 0 {
                buf.push(b);
            } else {
                buf.push(*carry | (b >> shift));
                *carry = b << (8 - shift);
            }
        };
        for h in first..first + count {
            let n = (reference_len(h) - 1) as usize;
            push(
                (DwgReferenceType::HardOwnership.code() << 4) | n as u8,
                &mut carry,
                &mut buf,
            );
            for &b in &h.to_be_bytes()[8 - n..] {
                push(b, &mut carry, &mut buf);
            }
            if buf.len() >= (1 << 16) - 16 {
                out(&buf, &mut crc)?;
                buf.clear();
            }
        }
        // The rest of the record: the remaining bits of the split byte, then as it was.
        if shift == 0 {
            buf.extend_from_slice(&data[at..]);
        } else {
            buf.push(carry | (data[at] & (0xFFu8 >> shift)));
            buf.extend_from_slice(&data[at + 1..]);
        }
        out(&buf, &mut crc)?;
        out(&crc.to_le_bytes(), &mut 0)?;
        Ok(written)
    }
}
