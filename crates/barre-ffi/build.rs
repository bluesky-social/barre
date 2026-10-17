//! Compiles `shim/shim.cpp` (see the comment at its top) into the library.

fn main() {
    println!("cargo:rerun-if-changed=shim/shim.cpp");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("shim/shim.cpp")
        .warnings(true)
        .compile("barre_ffi_shim");
}
