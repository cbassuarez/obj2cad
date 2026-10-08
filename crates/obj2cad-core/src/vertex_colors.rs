//! Per-vertex colors, stored compactly: vertices without a color take no space, and a
//! point cloud's colors (integers 0–255) take one byte per channel rather than an
//! optional float triple (16 bytes a point).

/// Per-vertex RGB colors (0..1), `None` for vertices without one.
#[derive(Debug, Default, Clone)]
pub struct VertexColors {
    len: usize,
    /// Consecutive runs, each starting where the one before ends.
    runs: Vec<(usize, Run)>,
}

#[derive(Debug, Clone)]
enum Run {
    /// Vertices without a color.
    None(usize),
    /// OBJ `v x y z r g b`: any value, some vertices perhaps without.
    Float(Vec<Option<[f32; 3]>>),
    /// RGB bytes, three per vertex.
    Bytes(Vec<u8>),
}

impl Run {
    fn len(&self) -> usize {
        match self {
            Run::None(n) => *n,
            Run::Float(v) => v.len(),
            Run::Bytes(b) => b.len() / 3,
        }
    }
}

impl VertexColors {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The color of vertex `v` (`None` without one, or past the end).
    pub fn get(&self, v: usize) -> Option<[f32; 3]> {
        if v >= self.len {
            return None;
        }
        let r = self.runs.partition_point(|(start, _)| *start <= v) - 1;
        let (start, run) = &self.runs[r];
        let i = v - start;
        match run {
            Run::None(_) => None,
            Run::Float(c) => c[i],
            Run::Bytes(b) => Some([0, 1, 2].map(|k| f32::from(b[i * 3 + k]) / 255.0)),
        }
    }

    pub(crate) fn push(&mut self, color: Option<[f32; 3]>) {
        match (self.runs.last_mut(), color) {
            (Some((_, Run::None(n))), None) => *n += 1,
            (Some((_, Run::Float(c))), _) => c.push(color),
            (_, None) => self.runs.push((self.len, Run::None(1))),
            (_, Some(_)) => self.runs.push((self.len, Run::Float(vec![color]))),
        }
        self.len += 1;
    }

    /// `n` vertices without a color.
    pub(crate) fn push_none(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        match self.runs.last_mut() {
            Some((_, Run::None(m))) => *m += n,
            _ => self.runs.push((self.len, Run::None(n))),
        }
        self.len += n;
    }

    /// Vertices colored by RGB bytes, three per vertex.
    pub(crate) fn push_bytes(&mut self, rgb: Vec<u8>) {
        let n = rgb.len() / 3;
        if n > 0 {
            self.runs.push((self.len, Run::Bytes(rgb)));
            self.len += n;
        }
    }

    /// Another document's colors after these (moved, not copied).
    pub(crate) fn append(&mut self, other: VertexColors) {
        for (_, run) in other.runs {
            let n = run.len();
            match (self.runs.last_mut(), run) {
                (Some((_, Run::None(m))), Run::None(k)) => *m += k,
                (_, run) => self.runs.push((self.len, run)),
            }
            self.len += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_read_back_as_pushed() {
        let mut c = VertexColors::default();
        c.push(None);
        c.push(None);
        c.push(Some([0.5, 0.25, 1.0]));
        c.push(None);
        c.push_none(3);
        c.push_bytes(vec![255, 0, 51, 1, 2, 3]);
        let mut d = VertexColors::default();
        d.push_none(2);
        d.push(Some([0.1, 0.2, 0.3]));
        c.append(d);
        let want = [
            None,
            None,
            Some([0.5, 0.25, 1.0]),
            None,
            None,
            None,
            None,
            Some([1.0, 0.0, 0.2]),
            Some([1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0]),
            None,
            None,
            Some([0.1, 0.2, 0.3]),
        ];
        assert_eq!(c.len(), want.len());
        for (v, w) in want.iter().enumerate() {
            assert_eq!(c.get(v), *w, "{v}");
        }
        assert_eq!(c.get(want.len()), None);
    }
}
