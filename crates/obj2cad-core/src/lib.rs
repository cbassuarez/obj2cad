//! Core model for obj2cad: a strict, lossless OBJ parser and the OBJ → CAD model step.
//!
//! "Lossless" means every coordinate keeps both its parsed `f64` and the exact text it
//! came from, and nothing in the file is dropped without a [`Diagnostic`] saying so.
//! "Strict" means anything whose meaning is ambiguous (bad numbers, out-of-range
//! indices, non-finite values) is a hard error, never a guess.

pub mod convert;
pub mod diag;
pub mod hash;
pub mod hints;
pub mod mtl;
pub mod obj;
pub mod output;
pub mod report;
pub mod synth;

pub use convert::{convert, CadModel, LayerMode, Omissions, Options, Units, UpAxis};
pub use diag::{Code, Diagnostic, Severity};
pub use obj::{parse, parse_with_progress, ErrorKind, ObjDocument, ParseError, ParseIssue};
pub use output::Meta;

/// Engine version, recorded in every output file and report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
