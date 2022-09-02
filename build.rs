use camino::Utf8Path;
use std::env;

fn main() {
    uniffi_bindgen::generate_component_scaffolding(
        Utf8Path::new("src/lipalightninglib.udl"),
        None,
        Some(Utf8Path::new(&env::var("OUT_DIR").unwrap())),
        false,
    )
    .unwrap();

    uniffi_bindgen::generate_bindings(
        Utf8Path::new("src/lipalightninglib.udl"),
        None,
        Vec::from(["swift"]),
        Some(Utf8Path::new("bindings/swift")),
        None,
        false,
    )
    .unwrap();

    uniffi_bindgen::generate_bindings(
        Utf8Path::new("src/lipalightninglib.udl"),
        None,
        Vec::from(["kotlin"]),
        Some(Utf8Path::new("bindings/kotlin")),
        None,
        false,
    )
    .unwrap();
}
