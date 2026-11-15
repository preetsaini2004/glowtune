fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++14")
        .include("signalsmith-stretch")
        .include(".")
        .file("src/wrapper.cpp")
        .warnings(false)
        .compile("signalsmith_stretch");
    std::fs::copy(
        "src/bindings.rs",
        std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("bindings.rs"),
    )
    .unwrap();
    for path in ["src", "signalsmith-stretch", "signalsmith-linear"] {
        println!("cargo:rerun-if-changed={path}");
    }
}
