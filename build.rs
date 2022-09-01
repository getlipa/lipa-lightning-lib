use std::env;
use std::process::Command;

fn main() {
    //uniffi_build::generate_scaffolding("./src/lipalightninglib.udl").unwrap();

    Command::new("uniffi-bindgen")
        .arg("scaffolding")
        .arg("src/lipalightninglib.udl")
        .arg("--no-format")
        .arg("--out-dir")
        .arg(env::var("OUT_DIR").unwrap())
        .output()
        .expect("Failed to generate scaffolding");

    /*Command::new("sh")
    .arg("scripts/generate-bindings.sh")
    .output()
    .expect("Failed to generate bindings");*/
}
