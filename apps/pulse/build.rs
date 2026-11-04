fn main() {
    println!("cargo:rerun-if-env-changed=PULSE_PYTHON");
    let lib = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../target/native/ntgcalls/lib");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../share/nyx/lib");
}
