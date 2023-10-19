use camino::Utf8Path;
use std::env;
use uniffi_bindgen::bindings::TargetLanguage;

fn main() {
    let udl_file = Utf8Path::new("src/lipalightninglib.udl");
    println!("cargo:rerun-if-changed={udl_file}");

    uniffi_bindgen::generate_component_scaffolding(
        udl_file,
        Some(Utf8Path::new(&env::var("OUT_DIR").unwrap())),
        false,
    )
    .unwrap();

    uniffi_bindgen::generate_bindings(
        udl_file,
        None,
        Vec::from([TargetLanguage::Swift]),
        Some(Utf8Path::new("bindings/swift")),
        None,
        None,
        false,
    )
    .unwrap();

    uniffi_bindgen::generate_bindings(
        udl_file,
        None,
        Vec::from([TargetLanguage::Kotlin]),
        Some(Utf8Path::new("bindings/kotlin")),
        None,
        None,
        false,
    )
    .unwrap();
}
