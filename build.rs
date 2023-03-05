use std::{env, path::PathBuf, process::Command};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    match target_os.as_str() {
        "macos" => compile_macos(),
        _ => {}
    }
}

fn compile_macos() {
    println!("cargo:rerun-if-changed=native/darwin/enumdisk.h");
    println!("cargo:rerun-if-changed=native/darwin/enumdisk.mm");
    println!("cargo:rerun-if-changed=native/darwin/Makefile");
    println!("cargo:rustc-link-search=native/darwin");
    println!("cargo:rustc-link-lib=caliguladarwin");

    let frameworks = ["Cocoa", "IOKit", "Foundation", "DiskArbitration"];
    for f in frameworks {
        println!("cargo:rustc-link-lib=framework={f}");
    }

    let make_code = Command::new("make")
        .current_dir("native/darwin")
        .spawn()
        .expect("Failed to start make on darwin code")
        .wait()
        .expect("Could not wait on make");
    assert!(make_code.success(), "Make failed to run!");

    let bindings = bindgen::Builder::default()
        .header("native/darwin/enumdisk.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("darwin_bindings.rs"))
        .expect("Couldn't write bindings!");
}
