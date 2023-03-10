use std::{env, path::PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    match target_os.as_str() {
        "macos" => compile_macos(),
        _ => {}
    }
}

fn compile_macos() {
    let srcs = ["native/darwin/enumdisk.mm", "native/darwin/REDiskList.m"];
    let headers = ["native/darwin/enumdisk.h", "native/darwin/REDiskList.h"];
    for file in srcs.iter().chain(headers.iter()) {
        println!("cargo:rerun-if-changed={}", file);
    }
    println!("cargo:rustc-link-search=native/darwin");
    println!("cargo:rustc-link-lib=caliguladarwin");

    let frameworks = ["Cocoa", "IOKit", "Foundation", "DiskArbitration"];
    for f in frameworks {
        println!("cargo:rustc-link-lib=framework={f}");
    }

    cc::Build::new()
        .files(srcs)
        .include("native/darwin")
        .flag("-F/Library/Frameworks")
        .compile("libcaliguladarwin.a");

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
