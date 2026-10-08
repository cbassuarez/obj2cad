//! The points of a file as it is read, for the viewer to build the scene from while the
//! engine parses. Display only: the preview built afterwards replaces them.

use obj2cad_core::bundle::Loading;
use obj2cad_core::color::TO_LINEAR;
use obj2cad_core::partial::STEP;

/// At most this many points are sent while reading (the preview shows up to 4 million).
pub const BUDGET: usize = 1_500_000;

/// Points without a color of their own, until the preview gives them their layer's:
/// graphite, #8f8a80.
const GRAPHITE: [u8; 3] = [0x8f, 0x8a, 0x80];

/// New points since the last report, thinned evenly and positioned relative to the first
/// point read (so `f32` keeps them precise).
#[derive(Default)]
pub struct Stream {
    budget: usize,
    sent: usize,
    file: String,
    /// Positions of the current file already looked at, and the thinning step for it.
    cursor: usize,
    stride: usize,
    pub origin: Option<[f64; 3]>,
    /// The new points (xyz) and their colors (linear light, as the preview's).
    pub points: Vec<f32>,
    pub colors: Vec<u8>,
}

impl Stream {
    pub fn new(budget: usize) -> Self {
        Stream {
            budget,
            ..Default::default()
        }
    }

    /// Take the points read since the last call. Small files (read in one step) send
    /// nothing: they are shown whole a moment later.
    pub fn take(&mut self, l: &Loading) {
        self.points.clear();
        self.colors.clear();
        if l.file != self.file {
            self.file = l.file.to_owned();
            self.cursor = 0;
            self.stride = 0;
        }
        let p = l.partial;
        let n = p.positions.len();
        if p.total <= STEP || n == 0 || self.cursor >= n {
            return;
        }
        if self.stride == 0 {
            // The whole file's points, estimated from those in the part read so far.
            let estimate = (n as f64 * p.total as f64 / p.done.max(1) as f64).ceil() as usize;
            let left = self.budget.saturating_sub(self.sent);
            if left == 0 {
                self.cursor = n;
                return;
            }
            self.stride = estimate.div_ceil(left).max(1);
        }
        let origin = *self.origin.get_or_insert(p.positions[0]);
        let linear = |c: f32| {
            (TO_LINEAR[(c * 255.0).round().clamp(0.0, 255.0) as usize] * 255.0).round() as u8
        };
        let graphite = GRAPHITE.map(|c| (TO_LINEAR[c as usize] * 255.0).round() as u8);
        let first = self.cursor.div_ceil(self.stride) * self.stride;
        for i in (first..n).step_by(self.stride) {
            if self.sent >= self.budget {
                break;
            }
            let q = p.positions[i];
            self.points
                .extend([0, 1, 2].map(|a| (q[a] - origin[a]) as f32));
            match p.color(i) {
                Some(c) => self.colors.extend(c.map(linear)),
                None => self.colors.extend(graphite),
            }
            self.sent += 1;
        }
        self.cursor = n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obj2cad_core::bundle::{self, Content, InputFile};

    fn stream(
        src: &[u8],
        name: &str,
        budget: usize,
    ) -> (Vec<f32>, Vec<u8>, usize, Option<[f64; 3]>) {
        let mut s = Stream::new(budget);
        let (mut points, mut colors, mut calls) = (Vec::new(), Vec::new(), 0);
        let files = vec![InputFile {
            path: name.into(),
            content: Content::Bytes(src),
            sha256: None,
            modified: None,
        }];
        bundle::load(files, "x", |l| {
            s.take(l);
            points.extend_from_slice(&s.points);
            colors.extend_from_slice(&s.colors);
            calls += 1;
        })
        .unwrap();
        (points, colors, calls, s.origin)
    }

    fn cloud(n: usize) -> String {
        (0..n)
            .map(|i| format!("{}.5 {}.25 7.125 {} 0 255\n", 1000 + i, i % 50, i % 256))
            .collect()
    }

    #[test]
    fn a_large_cloud_is_sent_as_it_is_read() {
        let src = cloud(400_000); // ~11 MB, several steps
        let (points, colors, calls, origin) = stream(src.as_bytes(), "scan.xyz", BUDGET);
        assert!(calls >= 3);
        assert_eq!(points.len(), 400_000 * 3, "every point, once, in order");
        assert_eq!(colors.len(), points.len());
        assert_eq!(origin, Some([1000.5, 0.25, 7.125]));
        assert_eq!(&points[3..6], &[1.0, 1.0, 0.0]);
        // Colored as read: the red column is i % 256, blue 255.
        assert_eq!(
            (colors[3 * 7], colors[3 * 7 + 1], colors[3 * 7 + 2]),
            (linear_of(7), 0, 255)
        );
    }

    fn linear_of(c: u8) -> u8 {
        (TO_LINEAR[c as usize] * 255.0).round() as u8
    }

    #[test]
    fn thinned_evenly_within_the_budget() {
        let src = cloud(400_000);
        let (points, _, _, _) = stream(src.as_bytes(), "scan.xyz", 100_000);
        let n = points.len() / 3;
        assert!((80_000..=100_000).contains(&n), "{n} points sent");
        // Evenly: the gaps between the x values sent are all the same.
        let step = points[3] - points[0];
        assert!(step >= 4.0);
        assert!(points
            .chunks(3)
            .collect::<Vec<_>>()
            .windows(2)
            .all(|w| w[1][0] - w[0][0] == step));
    }

    #[test]
    fn small_files_send_nothing() {
        let (points, _, calls, _) = stream(cloud(1000).as_bytes(), "scan.xyz", BUDGET);
        assert!(calls >= 1);
        assert!(points.is_empty());
    }
}
