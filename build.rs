use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=user_bin/src/bin/cat2.rs");
    println!("cargo:rerun-if-changed=user_bin/src/bin/wc.rs");
    println!("cargo:rerun-if-changed=user_bin/Cargo.toml");
    println!("cargo:rerun-if-changed=user_bin/.cargo/config.toml");

    let cargo = env::var("CARGO").expect("CARGO env not set");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target = "riscv64gc-unknown-none-elf";

    // Build all user binaries (cat2 and wc)
    let user_manifest = manifest_dir.join("user_bin/Cargo.toml");
    let status = Command::new(&cargo)
        .current_dir(&manifest_dir)
        .args([
            "build",
            "--release",
            "--manifest-path",
            user_manifest.to_str().unwrap(),
            "--target",
            target,
            "-Z",
            "build-std=core,compiler_builtins",
            "-Z",
            "build-std-features=compiler-builtins-mem",
        ])
        .status()
        .expect("failed to build user binaries");

    if !status.success() {
        panic!("building user_bin failed");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::create_dir_all(&out_dir).expect("failed to create OUT_DIR");

    // Copy cat2 binary
    let cat2_binary = manifest_dir
        .join("user_bin")
        .join("target")
        .join(target)
        .join("release")
        .join("cat2");
    let cat2_out = out_dir.join("cat2.bin");
    fs::copy(&cat2_binary, &cat2_out).expect("failed to copy cat2 binary");

    // Copy wc binary
    let wc_binary = manifest_dir
        .join("user_bin")
        .join("target")
        .join(target)
        .join("release")
        .join("wc");
    let wc_out = out_dir.join("wc.bin");
    fs::copy(&wc_binary, &wc_out).expect("failed to copy wc binary");
}
