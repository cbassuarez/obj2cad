//! What a parser has read so far, so an app can show a file while it is read.

use crate::vertex_colors::VertexColors;

/// How often the parsers report, in bytes of the file read.
pub const STEP: usize = 4 << 20;

/// A file part-way through parsing (see [`crate::obj::parse_with_progress`] and
/// [`crate::xyz::parse_with_progress`]).
pub struct Partial<'a> {
    /// Bytes of the file read so far, and its size.
    pub done: usize,
    pub total: usize,
    /// Every position read so far, in the file's own axes.
    pub positions: &'a [[f64; 3]],
    pub(crate) colors: Colors<'a>,
}

pub(crate) enum Colors<'a> {
    None,
    /// One per position (OBJ `v x y z r g b`).
    Vertex(&'a VertexColors),
    /// A point cloud's extra columns, `width` per point, with red, green and blue
    /// (0..=255) at `at` while every value read so far looks like a color.
    Columns {
        extra: &'a [u8],
        width: usize,
        at: usize,
    },
}

impl Partial<'_> {
    /// The color of position `i` (RGB, 0..1) as far as the file is known so far: a cloud's
    /// columns are only known to be colors once the whole file is read.
    pub fn color(&self, i: usize) -> Option<[f32; 3]> {
        match self.colors {
            Colors::None => None,
            Colors::Vertex(c) => c.get(i),
            Colors::Columns { extra, width, at } => {
                let e = extra.get(i * width + at..i * width + at + 3)?;
                Some([0, 1, 2].map(|k| f32::from(e[k]) / 255.0))
            }
        }
    }
}
