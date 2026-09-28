use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=boot");
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("boot_scripts.rs");

    let mut entries = Vec::new();
    let boot_dir = Path::new("boot");
    if let Ok(read_dir) = fs::read_dir(boot_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "hob") {
                if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                    entries.push(file_name.to_string());
                }
            }
        }
    }
    entries.sort();

    let mut code = String::new();
    code.push_str("pub const BOOT_SCRIPTS: &[(&str, &str)] = &[\n");
    for name in &entries {
        code.push_str(&format!(
            "    (\"{name}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/boot/{name}\"))),\n"
        ));
    }
    code.push_str("];\n");

    fs::write(dest_path, code).unwrap();
}
