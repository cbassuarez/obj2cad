//! Every shape in tests/fixtures/curves (tools/fixtures/curves.py) gives exactly the
//! surfaces it was built from, and nothing else.

use obj2cad_core::{convert, parse, Options, UpAxis};

fn kinds(name: &str, up: UpAxis) -> Vec<(&'static str, usize)> {
    let path = format!(
        "{}/../../tests/fixtures/curves/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = parse(&std::fs::read(path).unwrap()).unwrap();
    let model = convert(
        &doc,
        None,
        Options {
            up_axis: up,
            ..Options::default()
        },
    );
    let mut k: Vec<(&'static str, usize)> = obj2cad_curves::recognize(&model)
        .iter()
        .map(|f| (f.region.kind, f.region.faces.len()))
        .collect();
    k.sort();
    k
}

#[test]
fn each_shape_gives_its_surfaces() {
    for up in [UpAxis::AsIs, UpAxis::YUpToZUp] {
        assert_eq!(kinds("cylinder_capped.obj", up), [("cylinder", 32)]);
        assert_eq!(kinds("cone_frustum.obj", up), [("cone", 32)]);
        assert_eq!(kinds("half_cylinder.obj", up), [("cylinder", 32)]);
        assert_eq!(kinds("sphere_uv.obj", up), [("sphere", 288)]);
        assert_eq!(kinds("torus.obj", up), [("torus", 648)]);
        assert_eq!(
            kinds("capsule.obj", up),
            [("cylinder", 24), ("sphere", 144), ("sphere", 144)]
        );
        assert_eq!(kinds("boss.obj", up), [("cylinder", 32), ("torus", 128)]);
    }
}

#[test]
fn noise_and_flat_shapes_stay_faceted() {
    assert!(kinds("noisy_cylinder.obj", UpAxis::AsIs).is_empty());
    assert!(kinds("box.obj", UpAxis::AsIs).is_empty());
}

#[test]
fn surfaces_are_written_next_to_the_unchanged_mesh() {
    let path = format!(
        "{}/../../tests/fixtures/curves/capsule.obj",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = parse(&std::fs::read(path).unwrap()).unwrap();
    let mut model = convert(&doc, None, Options::default());
    let before = obj2cad_core::hash::parity_hash(&model);
    assert_eq!(obj2cad_curves::add_to(&mut model), 3);
    assert_eq!(obj2cad_core::hash::parity_hash(&model), before);
    assert_eq!(model.layers.last().unwrap().name, "Curves");
    assert!(model.surfaces.iter().all(|s| s.body.validate(1e-5).is_ok()));
}
