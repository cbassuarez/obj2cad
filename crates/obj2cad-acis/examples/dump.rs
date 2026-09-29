// Write every sample as SAB and SAT into a folder (for checking with other readers).
fn main() {
    let dir = std::env::args().nth(1).expect("usage: dump <dir>");
    for (name, body) in obj2cad_acis::samples::all() {
        std::fs::write(
            format!("{dir}/{name}.sab"),
            obj2cad_acis::sab::write(&body, "obj2cad", 0.0),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/{name}.sat"),
            obj2cad_acis::sat::write(&body, "obj2cad", 0.0),
        )
        .unwrap();
    }
}
