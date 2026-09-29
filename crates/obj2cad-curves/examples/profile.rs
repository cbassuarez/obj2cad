// Time recognition on an OBJ: cargo run --release -p obj2cad-curves --example profile -- file.obj
fn main() {
    let path = std::env::args().nth(1).expect("usage: profile <file.obj>");
    let src = std::fs::read(&path).unwrap();
    let doc = obj2cad_core::parse(&src).unwrap();
    let model = obj2cad_core::convert(&doc, None, obj2cad_core::Options::default());
    let t = std::time::Instant::now();
    let found = obj2cad_curves::recognize(&model);
    let kinds: Vec<(&str, usize)> = found
        .iter()
        .map(|f| (f.region.kind, f.region.faces.len()))
        .collect();
    println!("{} faces: {kinds:?} in {:?}", doc.faces.len(), t.elapsed());
}
