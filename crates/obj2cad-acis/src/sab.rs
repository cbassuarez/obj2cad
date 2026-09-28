//! SAB: binary ACIS, version 21800 with an ASM header, as DXF R2013+ (ACDSDATA section)
//! and DWG R2013+ store it. This is the layout ezdxf verified against Autodesk's viewer.

use crate::records::{ctime, records, Tok};
use crate::Body;

const VERSION: i32 = 21800;
const ACIS_VERSION: &str = "ACIS 218.00 NT";
const ASM_VERSION: &str = "208.0.4.7009";

mod tag {
    pub const INT: u8 = 0x04;
    pub const DOUBLE: u8 = 0x06;
    pub const STR: u8 = 0x07;
    pub const TRUE: u8 = 0x0A;
    pub const FALSE: u8 = 0x0B;
    pub const POINTER: u8 = 0x0C;
    pub const ENTITY_TYPE: u8 = 0x0D;
    pub const ENTITY_TYPE_EX: u8 = 0x0E;
    pub const RECORD_END: u8 = 0x11;
    pub const POSITION: u8 = 0x13;
    pub const DIRECTION: u8 = 0x14;
}

fn str_tag(out: &mut Vec<u8>, t: u8, s: &str) {
    out.push(t);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}

fn double(out: &mut Vec<u8>, v: f64) {
    out.push(tag::DOUBLE);
    out.extend_from_slice(&v.to_le_bytes());
}

fn int(out: &mut Vec<u8>, t: u8, v: i64) {
    out.push(t);
    out.extend_from_slice(&(v as i32).to_le_bytes());
}

fn entity_type(out: &mut Vec<u8>, name: &str) {
    let parts: Vec<&str> = name.split('-').collect();
    for p in &parts[..parts.len() - 1] {
        str_tag(out, tag::ENTITY_TYPE_EX, p);
    }
    str_tag(out, tag::ENTITY_TYPE, parts[parts.len() - 1]);
}

/// Encode `body`. `created_unix` dates it (deterministic output for a given date).
pub fn write(body: &Body, product: &str, created_unix: f64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"ACIS BinaryFile");
    for v in [VERSION, 0, 2, 12] {
        // version, records (0: not needed), entities (body + ASM header), flags
        out.extend_from_slice(&v.to_le_bytes());
    }
    str_tag(&mut out, tag::STR, product);
    str_tag(&mut out, tag::STR, ACIS_VERSION);
    str_tag(&mut out, tag::STR, &ctime(created_unix));
    double(&mut out, 1.0); // units: millimeters per model unit (not used by AutoCAD)
    double(&mut out, 1e-6); // resolution (the SAT header prints it as 9.9999999999999995e-007)
    double(&mut out, 1e-10);
    for r in records(body, Some(ASM_VERSION)) {
        entity_type(&mut out, r.name);
        int(&mut out, tag::POINTER, -1); // attributes
        int(&mut out, tag::INT, -1); // id
        for t in &r.toks {
            match *t {
                Tok::Ptr(p) => int(&mut out, tag::POINTER, p),
                Tok::SabInt(v) => int(&mut out, tag::INT, i64::from(v)),
                Tok::Double(v) => double(&mut out, v),
                Tok::Pos(v) | Tok::Dir(v) => {
                    out.push(if matches!(t, Tok::Pos(_)) {
                        tag::POSITION
                    } else {
                        tag::DIRECTION
                    });
                    for c in v {
                        out.extend_from_slice(&c.to_le_bytes());
                    }
                }
                Tok::Bool(v, _, _) => out.push(if v { tag::TRUE } else { tag::FALSE }),
                // F (finite, followed by the value) and I (infinite).
                Tok::Interval(Some(v)) => {
                    out.push(tag::TRUE);
                    double(&mut out, v);
                }
                Tok::Interval(None) => out.push(tag::FALSE),
                Tok::Str(s) => str_tag(&mut out, tag::STR, s),
            }
        }
        out.push(tag::RECORD_END);
    }
    // End-of-ASM-data
    str_tag(&mut out, tag::ENTITY_TYPE_EX, "End");
    str_tag(&mut out, tag::ENTITY_TYPE_EX, "of");
    str_tag(&mut out, tag::ENTITY_TYPE_EX, "ASM");
    str_tag(&mut out, tag::ENTITY_TYPE, "data");
    out
}
