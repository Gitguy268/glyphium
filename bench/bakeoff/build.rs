//! With the `ghostty` feature, generates bindings for libghostty-vt and links it statically.
//! `GHOSTTY_VT_DIR` points at a `zig build -Demit-lib-vt` output directory (default:
//! `.tools/ghostty/zig-out`, or `zig-out-wasm` for wasm32, built with
//! `-Dtarget=wasm32-freestanding --prefix zig-out-wasm`). libclang comes from
//! `LIBCLANG_PATH`, or the Xcode Command Line Tools on macOS.

use std::env;
#[cfg(feature = "ghostty")]
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=GHOSTTY_VT_DIR");
    if env::var_os("CARGO_FEATURE_GHOSTTY").is_none() {
        return;
    }
    #[cfg(feature = "ghostty")]
    ghostty();
}

#[cfg(feature = "ghostty")]
fn ghostty() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let wasm = env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32");
    let out = if wasm { "zig-out-wasm" } else { "zig-out" };
    let dir = env::var_os("GHOSTTY_VT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../.tools/ghostty").join(out));
    // The wasm install has no headers; they are the same as the native ones.
    let include = manifest.join("../../.tools/ghostty/include");
    let header = include.join("ghostty/vt.h");
    assert!(header.exists(), "libghostty-vt not found at {}; set GHOSTTY_VT_DIR", dir.display());
    println!("cargo:rerun-if-changed={}", header.display());
    println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    if env::var_os("LIBCLANG_PATH").is_none() && cfg!(target_os = "macos") {
        // SAFETY: build scripts are single-threaded.
        unsafe { env::set_var("LIBCLANG_PATH", "/Library/Developer/CommandLineTools/usr/lib") };
    }
    bindgen::Builder::default()
        .header(header.to_str().unwrap())
        .clang_arg(format!("-I{}", include.display()))
        .clang_args(if wasm { &["--target=wasm32-unknown-unknown", "-ffreestanding"][..] } else { &[] })
        .allowlist_function("ghostty_.*")
        .allowlist_type("Ghostty.*")
        .allowlist_var("GHOSTTY_.*")
        .prepend_enum_name(false)
        .layout_tests(false)
        .generate()
        .expect("bindgen libghostty-vt")
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("ghostty_vt.rs"))
        .unwrap();
}
