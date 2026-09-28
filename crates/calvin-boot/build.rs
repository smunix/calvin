use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=boot");
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("boot_scripts.rs");

    let mut entries: Vec<String> = fs::read_dir("boot")
        .into_iter()
        .flat_map(|read_dir| read_dir.flatten())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hob"))
        .filter_map(|path| path.file_name().and_then(|s| s.to_str()).map(ToString::to_string))
        .collect();
    entries.sort();

    let body = entries
        .iter()
        .map(|name| {
            format!(
                "    (\"{name}\", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/boot/{name}\"))),\n"
            )
        })
        .collect::<String>();
    let code = format!("pub const BOOT_SCRIPTS: &[(&str, &str)] = &[\n{body}];\n");

    fs::write(dest_path, code).unwrap();
}
