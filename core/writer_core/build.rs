#[allow(clippy::unwrap_used)]
fn main() {
    uniffi::generate_scaffolding("./src/api.udl").unwrap();

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let api_path = std::path::Path::new(&out_dir).join("api.uniffi.rs");
    if api_path.exists() {
        let content = std::fs::read_to_string(&api_path).unwrap();
        let patched = content.replace(
            "/// Export info about the UDL while used to create us\n/// See `uniffi_bindgen::macro_metadata` for how this is used.",
            "// /// Export info about the UDL while used to create us\n// /// See `uniffi_bindgen::macro_metadata` for how this is used.",
        );
        if patched != content {
            std::fs::write(&api_path, patched).unwrap();
        }
    }
}
