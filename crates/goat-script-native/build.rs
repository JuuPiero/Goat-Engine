fn main() {
    println!("cargo:include={}/include", std::env::var("CARGO_MANIFEST_DIR").unwrap());
    #[cfg(feature = "examples")]
    {
        println!("cargo:rerun-if-changed=examples/wobble.c");
        println!("cargo:rerun-if-changed=examples/orbit.cpp");
        println!("cargo:rerun-if-changed=include/goat.h");
        cc::Build::new().file("examples/wobble.c").include("include").warnings(true).compile("goat_example_c");
        cc::Build::new().cpp(true).file("examples/orbit.cpp").include("include").warnings(true).compile("goat_example_cpp");
    }
}
