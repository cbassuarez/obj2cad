//! SAT: text ACIS, version 7.0 (the layout DXF R2000–R2010 store). Used for inspection
//! and for readers that only take text.

use crate::records::{ctime, records, Tok};
use crate::Body;
use std::fmt::Write;

fn num(out: &mut String, v: f64) {
    // Shortest text that parses back to exactly `v` (C `strtod` reads it).
    let _ = write!(out, " {v:?}");
}

pub fn write(body: &Body, product: &str, created_unix: f64) -> String {
    let mut out = String::new();
    let date = ctime(created_unix);
    let _ = writeln!(out, "700 0 1 0");
    let _ = writeln!(
        out,
        "@{} {product} @12 ACIS 32.0 NT @{} {date}",
        product.len(),
        date.len()
    );
    let _ = writeln!(out, "1 9.9999999999999995e-007 1e-010");
    for r in records(body, None) {
        out.push_str(r.name);
        out.push_str(" $-1 -1");
        for t in &r.toks {
            match *t {
                Tok::Ptr(p) => {
                    let _ = write!(out, " ${p}");
                }
                Tok::SabInt(_) => {}
                Tok::Double(v) => num(&mut out, v),
                Tok::Pos(v) | Tok::Dir(v) => v.iter().for_each(|&c| num(&mut out, c)),
                Tok::Bool(v, yes, no) => {
                    let _ = write!(out, " {}", if v { yes } else { no });
                }
                Tok::Interval(Some(v)) => {
                    out.push_str(" F");
                    num(&mut out, v);
                }
                Tok::Interval(None) => out.push_str(" I"),
                Tok::Str(s) => {
                    let _ = write!(out, " @{} {s}", s.len());
                }
            }
        }
        out.push_str(" #\n");
    }
    out.push_str("End-of-ACIS-data\n");
    out
}
