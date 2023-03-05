use std::{path::PathBuf, env};

fn main() {
    println!("cargo:rustc-link-search=native");
    println!("cargo:rerun-if-changed=native/enumdisk.h");

    let bindings = bindgen::Builder::default()
        .header("native/enumdisk.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
