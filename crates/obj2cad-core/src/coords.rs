//! The exact source text of every coordinate, stored compactly.
//!
//! Almost every coordinate in a real file is a plain decimal (`-12.345600`) that can be
//! rebuilt from its value and its number of decimals, so that is all that is kept: one
//! byte per coordinate. A coordinate is kept that way only when rebuilding it gives back
//! its exact text (checked as it is stored); any other text (`1e3`, `+5`, `.5`, `007`,
//! more than 15 digits) is kept as written. A point cloud of 45 million points needs
//! 135 MB here rather than the 1.7 GB its text and offsets took.

use crate::obj::POW10;
use std::fmt;
use std::ops::Deref;

/// A coordinate whose text is kept as written.
const AS_WRITTEN: u8 = u8::MAX;

/// Longest rebuilt text: a sign, `0.`, then up to 22 decimals.
const BUILT: usize = 32;

#[derive(Debug, Default, Clone)]
pub struct Coords {
    /// Per coordinate (vertex × 3 + axis): its number of decimals, or [`AS_WRITTEN`].
    codes: Vec<u8>,
    /// The coordinates kept as written, in order: (coordinate, end of its text in `text`).
    written: Vec<(usize, u32)>,
    text: Vec<u8>,
}

/// The text is too large to index (more than 4 GB of coordinates kept as written).
#[derive(Debug)]
pub(crate) struct TooLarge;

impl Coords {
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    pub(crate) fn reserve_exact(&mut self, coordinates: usize) {
        self.codes.reserve_exact(coordinates);
    }

    pub(crate) fn shrink_to(&mut self, coordinates: usize) {
        self.codes.shrink_to(coordinates);
    }

    /// Add the next coordinate: its text, and the value it was read as.
    pub(crate) fn push(&mut self, text: &[u8], value: f64) -> Result<(), TooLarge> {
        match decimals(text, value) {
            Some(k) => {
                self.codes.push(k);
                Ok(())
            }
            None => self.push_written(text),
        }
    }

    /// Add a coordinate with no text (a vertex that couldn't be read; the document is
    /// discarded, this only keeps indices aligned).
    pub(crate) fn push_empty(&mut self) {
        // An empty text never overflows: the end is the current end.
        let _ = self.push_written(b"");
    }

    fn push_written(&mut self, text: &[u8]) -> Result<(), TooLarge> {
        self.text.extend_from_slice(text);
        let end = u32::try_from(self.text.len()).map_err(|_| TooLarge)?;
        self.written.push((self.codes.len(), end));
        self.codes.push(AS_WRITTEN);
        Ok(())
    }

    /// The exact text of coordinate `k`, whose value is `value`.
    pub fn get(&self, k: usize, value: f64) -> Token<'_> {
        let code = self.codes[k];
        if code != AS_WRITTEN {
            return Token::build(value, code);
        }
        let at = self.written.partition_point(|&(c, _)| c < k);
        let start = if at == 0 { 0 } else { self.written[at - 1].1 };
        Token(Inner::Text(
            &self.text[start as usize..self.written[at].1 as usize],
        ))
    }

    /// Several documents' coordinates, one after the other (see [`crate::bundle`]): the
    /// largest are moved rather than copied.
    pub(crate) fn concat(parts: Vec<Coords>) -> Result<Coords, TooLarge> {
        let mut k0 = 0;
        let mut t0 = 0usize;
        let mut starts = Vec::with_capacity(parts.len());
        for c in &parts {
            starts.push((k0, t0));
            k0 += c.codes.len();
            t0 += c.text.len();
        }
        u32::try_from(t0).map_err(|_| TooLarge)?;
        let (mut codes, mut written, mut text) = (Vec::new(), Vec::new(), Vec::new());
        for c in parts {
            codes.push(c.codes);
            written.push(c.written);
            text.push(c.text);
        }
        Ok(Coords {
            codes: concat(codes, |_, _| {}),
            written: concat(written, |p, (c, end)| {
                *c += starts[p].0;
                *end += starts[p].1 as u32;
            }),
            text: concat(text, |_, _| {}),
        })
    }
}

/// Vectors one after the other, each element passed through `shift(part, element)`. The
/// longest is moved and grown in place where its allocation allows, rather than copied:
/// a point cloud's arrays are most of the memory a drawing takes.
pub(crate) fn concat<T: Copy>(
    mut parts: Vec<Vec<T>>,
    mut shift: impl FnMut(usize, &mut T),
) -> Vec<T> {
    let Some(big) = (0..parts.len()).max_by_key(|&p| parts[p].len()) else {
        return Vec::new();
    };
    let mut out = std::mem::take(&mut parts[big]);
    out.iter_mut().for_each(|x| shift(big, x));
    let before: usize = parts[..big].iter().map(Vec::len).sum();
    let after: usize = parts[big + 1..].iter().map(Vec::len).sum();
    if before + after == 0 {
        return out;
    }
    out.reserve_exact(before + after);
    if before > 0 {
        let n = out.len();
        out.resize(n + before, out[0]);
        out.copy_within(0..n, before);
        let mut at = 0;
        for (p, part) in parts[..big].iter().enumerate() {
            for (dst, &x) in out[at..at + part.len()].iter_mut().zip(part) {
                *dst = x;
                shift(p, dst);
            }
            at += part.len();
        }
    }
    for (p, part) in parts.into_iter().enumerate().skip(big + 1) {
        out.extend(part.into_iter().map(|mut x| {
            shift(p, &mut x);
            x
        }));
    }
    out
}

/// The number of decimals of `text` when it is the plain decimal that [`Token::build`]
/// rebuilds from `value`, else `None`.
fn decimals(text: &[u8], value: f64) -> Option<u8> {
    let digits = text.strip_prefix(b"-").unwrap_or(text);
    if (text.len() != digits.len()) != value.is_sign_negative() {
        return None;
    }
    let (int, frac) = match memchr::memchr(b'.', digits) {
        Some(i) => (&digits[..i], Some(&digits[i + 1..])),
        None => (digits, None),
    };
    // No leading zeros (but `0.5`), no `5.`, no `.5`.
    if int.is_empty() || (int.len() > 1 && int[0] == b'0') || frac.is_some_and(<[u8]>::is_empty) {
        return None;
    }
    let frac = frac.unwrap_or_default();
    if frac.len() >= POW10.len() {
        return None;
    }
    let mut m = 0u64;
    for &b in int.iter().chain(frac) {
        if !b.is_ascii_digit() || m >= 100_000_000_000_000 {
            return None;
        }
        m = m * 10 + u64::from(b - b'0');
    }
    // With at most 15 significant digits the value is within 0.25 of m once scaled back,
    // so this recovers m exactly; checked anyway, it is what `build` relies on.
    let k = frac.len();
    ((value.abs() * POW10[k]).round() == m as f64).then_some(k as u8)
}

/// A coordinate's exact text (see [`Coords::get`]).
#[derive(Clone, Copy)]
pub struct Token<'a>(Inner<'a>);

#[derive(Clone, Copy)]
enum Inner<'a> {
    Text(&'a [u8]),
    Built { buf: [u8; BUILT], len: u8 },
}

impl Token<'_> {
    /// `value` written with `k` decimals.
    fn build(value: f64, k: u8) -> Self {
        let k = usize::from(k);
        let mut m = (value.abs() * POW10[k]).round() as u64;
        // Digits backwards from the end of the buffer, then the sign.
        let mut buf = [0u8; BUILT];
        let mut at = BUILT;
        let mut written = 0;
        while m > 0 || written <= k {
            if written == k && k > 0 {
                at -= 1;
                buf[at] = b'.';
            }
            at -= 1;
            buf[at] = b'0' + (m % 10) as u8;
            m /= 10;
            written += 1;
        }
        if value.is_sign_negative() {
            at -= 1;
            buf[at] = b'-';
        }
        let len = BUILT - at;
        buf.copy_within(at.., 0);
        Token(Inner::Built {
            buf,
            len: len as u8,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        match &self.0 {
            Inner::Text(t) => t,
            Inner::Built { buf, len } => &buf[..usize::from(*len)],
        }
    }

    pub fn as_str(&self) -> &str {
        // Only ASCII number tokens that parsed successfully are stored or built.
        std::str::from_utf8(self.as_bytes()).expect("coordinate text is ASCII")
    }
}

impl Deref for Token<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for Token<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<&str> for Token<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> (String, bool) {
        let v: f64 = text.parse().unwrap();
        let mut c = Coords::default();
        c.push(text.as_bytes(), v).unwrap();
        (c.get(0, v).to_string(), c.codes[0] != AS_WRITTEN)
    }

    #[test]
    fn concat_moves_the_longest_and_keeps_order() {
        for big in 0..3 {
            let parts: Vec<Vec<u32>> = (0..3)
                .map(|p| {
                    (0..if p == big { 50 } else { 3 + p as u32 })
                        .map(|i| p as u32 * 100 + i)
                        .collect()
                })
                .collect();
            let want: Vec<u32> = parts
                .iter()
                .enumerate()
                .flat_map(|(p, v)| v.iter().map(move |x| x + p as u32))
                .collect();
            assert_eq!(concat(parts, |p, x| *x += p as u32), want);
        }
        assert!(concat::<u8>(Vec::new(), |_, _| {}).is_empty());
    }

    #[test]
    fn plain_decimals_are_rebuilt_and_everything_else_kept() {
        for (text, compact) in [
            ("0", true),
            ("-0", true),
            ("-0.000000", true),
            ("0.770493", true),
            ("90.721939", true),
            ("13.380070", true),
            ("-1234567.12345678", true),
            ("0.0000000000000000001234", true),
            ("100", true),
            ("1e3", false),
            ("+5", false),
            ("5.", false),
            ("007", false),
            ("0.12345678901234567", false),
            ("1234567890123456", false),
            ("-0.1E-3", false),
        ] {
            assert_eq!(round_trip(text), (text.to_owned(), compact), "{text}");
        }
    }

    #[test]
    fn every_decimal_of_up_to_15_digits_rebuilds() {
        // Values spread over many magnitudes and decimal counts.
        let mut x = 0x2545F4914F6CDD1Du64;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let k = (x % 16) as usize;
            let digits = 1 + (x >> 8) % 15;
            let m = (x >> 16) % 10u64.pow(digits as u32);
            let s = format!("{}{}", if x & 1 == 1 { "-" } else { "" }, m);
            let s = if k == 0 {
                s
            } else {
                let neg = s.starts_with('-');
                let d = s.trim_start_matches('-');
                let d = format!("{d:0>width$}", width = k + 1);
                let (a, b) = d.split_at(d.len() - k);
                format!("{}{a}.{b}", if neg { "-" } else { "" })
            };
            let v: f64 = s.parse().unwrap();
            let mut c = Coords::default();
            c.push(s.as_bytes(), v).unwrap();
            assert_eq!(c.get(0, v).as_str(), s);
        }
    }

    #[test]
    fn texts_kept_as_written_are_found_among_rebuilt_ones() {
        let mut c = Coords::default();
        let all = ["1.5", "1e3", "2", "+4", "-0.25", ".5"];
        for t in all {
            c.push(t.as_bytes(), t.parse().unwrap()).unwrap();
        }
        c.push_empty();
        for (k, t) in all.iter().enumerate() {
            assert_eq!(c.get(k, t.parse().unwrap()), *t);
        }
        assert_eq!(c.get(6, 0.0), "");

        let mut front = Coords::default();
        front.push(b"+1", 1.0).unwrap();
        front.push(b"3.5", 3.5).unwrap();
        let mut back = Coords::default();
        back.push(b"7e0", 7.0).unwrap();
        let c = Coords::concat(vec![front, c, back]).unwrap();
        let values = [1.0, 3.5, 1.5, 1e3, 2.0, 4.0, -0.25, 0.5, 0.0, 7.0];
        let texts = [
            "+1", "3.5", "1.5", "1e3", "2", "+4", "-0.25", ".5", "", "7e0",
        ];
        for k in 0..values.len() {
            assert_eq!(c.get(k, values[k]), texts[k], "{k}");
        }
    }
}
