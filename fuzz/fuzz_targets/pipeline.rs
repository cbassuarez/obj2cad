//! Anything that parses must convert, hash and write (both encodings) without panicking.
#![no_main]
use libfuzzer_sys::fuzz_target;
use obj2cad_core::{convert, LayerMode, Options, UpAxis};

fuzz_target!(|data: &[u8]| {
    let Some((&flags, src)) = data.split_first() else { return };
    let Ok(doc) = obj2cad_core::parse(src) else { return };
    let options = Options {
        up_axis: if flags & 1 == 1 { UpAxis::YUpToZUp } else { UpAxis::AsIs },
        layer_mode: [LayerMode::Objects, LayerMode::Groups, LayerMode::Materials, LayerMode::Single][(flags >> 1 & 3) as usize],
        keep_loose_points: flags & 8 == 8,
        ..Options::default()
    };
    let model = convert(&doc, None, options);
    let _ = obj2cad_core::hash::parity_hash(&model);
    let meta = obj2cad_dxf::Meta { properties: &[], fingerprint_seed: "fuzz", created_unix: None };
    let _ = obj2cad_dxf::write(&model, &meta, obj2cad_dxf::Format::Ascii);
    let _ = obj2cad_dxf::write(&model, &meta, obj2cad_dxf::Format::Binary);
});
