//! obj2cad: model-space entities streamed into a DWG as it is written.
//!
//! A scan's point cloud is tens of millions of POINTs. As acadrust entities, with the
//! object records, the handle map and the duplicate checks the writer keeps for each,
//! they take tens of gigabytes; streamed, each point is filled into one reusable
//! [`Point`], encoded by the same code as any other POINT, and its record goes straight
//! into the AcDbObjects section, which is compressed and written a page at a time.
//!
//! The layout: the document's objects, then the streamed POINTs (handles after every
//! other object's, in order), then the model-space BLOCK_HEADER, which owns the
//! document's entities and then the points (written last, its list of points spliced
//! into it as it goes out). The handle map stores each point as the size of its record.

use crate::entities::Point;
use crate::types::Vector3;

/// Points to stream into model space (see [`crate::DwgWriter::write_streaming`]).
pub trait PointStream {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Point `i`'s layer (by name), color and location, into `point`. Other fields are
    /// as the previous call left them (the same `point` is reused for every point); the
    /// writer sets the handle and the owner.
    fn point(&self, i: usize, point: &mut Point);

    /// The points' extents, for the drawing's header.
    fn bounds(&self) -> Option<(Vector3, Vector3)>;
}

/// What the stream wrote: where each point's record is, for the handle map.
pub(crate) struct Streamed {
    /// The first point's handle; the others follow in order.
    pub first: u64,
    /// Each point's record length, in order.
    pub sizes: Vec<u8>,
    /// Where the first point's record starts in the AcDbObjects section.
    pub start: u64,
    /// The model-space BLOCK_HEADER's handle, and where its record starts.
    pub header: (u64, u64),
}

impl Streamed {
    /// The handle map entries of the stream (points, in handle order).
    pub fn entries(&self) -> impl Iterator<Item = (u64, i64)> + '_ {
        let mut at = self.start;
        self.sizes.iter().enumerate().map(move |(i, &size)| {
            let entry = (self.first + i as u64, at as i64);
            at += u64::from(size);
            entry
        })
    }
}
