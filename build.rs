use std::{env, fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let dir = root.join("skills/remarkable/references");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("skill references")
        .map(|e| e.expect("reference entry").path())
        .filter(|p| p.extension().is_some_and(|s| s == "md"))
        .collect();
    paths.sort();
    let mut code = String::from("pub const REFERENCES: &[(&str,&str)] = &[\n");
    for path in paths {
        let topic = path
            .file_stem()
            .expect("topic")
            .to_str()
            .expect("UTF-8 topic");
        code.push_str(&format!(
            "({topic:?},include_str!({:?})),\n",
            path.to_str().expect("UTF-8 path")
        ));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("output directory")).join("references.rs"),
        code,
    )
    .expect("embedded reference list");
}
