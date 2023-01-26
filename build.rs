use camino::Utf8Path;
use std::env;

fn main() {
    let udl_file = Utf8Path::new("src/lipalightninglib.udl");
    println!("cargo:rerun-if-changed={udl_file}");

    uniffi_bindgen::generate_component_scaffolding(
        udl_file,
        None,
        Some(Utf8Path::new(&env::var("OUT_DIR").unwrap())),
        false,
    )
    .unwrap();

    uniffi_bindgen::generate_bindings(
        udl_file,
        None,
        Vec::from(["swift"]),
        Some(Utf8Path::new("bindings/swift")),
        None,
        false,
    )
    .unwrap();

    uniffi_bindgen::generate_bindings(
        udl_file,
        None,
        Vec::from(["kotlin"]),
        Some(Utf8Path::new("bindings/kotlin")),
        None,
        false,
    )
    .unwrap();
}
