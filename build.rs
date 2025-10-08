use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=user_bin/src/main.rs");
    println!("cargo:rerun-if-changed=user_bin/Cargo.toml");
    println!("cargo:rerun-if-changed=user_bin/.cargo/config.toml");

    let cargo = env::var("CARGO").expect("CARGO env not set");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let user_manifest = manifest_dir.join("user_bin/Cargo.toml");
    let target = "riscv64imac-unknown-none-elf";

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
        .expect("failed to build cat2 user binary");

    if !status.success() {
        panic!("building user_bin failed");
    }

    let user_binary = manifest_dir
        .join("user_bin")
        .join("target")
        .join(target)
        .join("release")
        .join("cat2");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_file = out_dir.join("cat2.bin");

    fs::create_dir_all(&out_dir).expect("failed to create OUT_DIR");
    fs::copy(&user_binary, &out_file).expect("failed to copy cat2 binary");
}
