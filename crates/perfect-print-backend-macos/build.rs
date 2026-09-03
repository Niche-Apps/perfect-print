use std::env;

fn main() {
    println!("cargo:rerun-if-changed=src/poster_tiles.c");
    println!("cargo:rerun-if-changed=src/poster_tiles.h");
    println!("cargo:rerun-if-changed=src/native_print.m");

    // Pure C poster math is compiled on every platform so Linux CI can
    // exercise Scale → page-count contracts without AppKit.
    cc::Build::new()
        .file("src/poster_tiles.c")
        .include("src")
        .warnings(true)
        .compile("perfect_print_poster_tiles");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/native_print.m")
            .include("src")
            .flag("-fobjc-arc")
            .compile("perfect_print_native_macos");

        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Quartz");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
}
