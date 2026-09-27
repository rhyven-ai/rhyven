use std::{env, fs, path::Path};

fn collect(directory: &Path, root: &Path, output: &mut String) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .expect("read license directory")
        .map(|entry| entry.expect("read license entry").path())
        .collect();
    entries.sort();
    for path in entries {
        assert!(!path.is_symlink(), "License inputs must not be symlinks");
        if path.is_dir() {
            collect(&path, root, output);
        } else {
            let name = path.strip_prefix(root).unwrap().to_string_lossy();
            let text = fs::read_to_string(&path).expect("read license text");
            output.push_str(&format!("===== {name} =====\n{text}\n\n"));
        }
    }
}

fn main() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).join("../../third-party-notices");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut text = String::new();
    collect(&root, &root, &mut text);
    assert!(!text.is_empty(), "Dependency license notices are required");
    let output = env::var_os("OUT_DIR").unwrap();
    fs::write(Path::new(&output).join("THIRD_PARTY_NOTICES.txt"), text)
        .expect("write bundled license notices");
}
