fn main() {
    // This build.rs exists to satisfy Cargo's expectations, but the actual
    // building of user binaries is handled by the parent crate's build.rs.
    // The parent build.rs compiles these binaries with the correct RISC-V
    // target and copies them to the appropriate output directory.

    // Tell Cargo to rebuild if the library source changes
    println!("cargo:rerun-if-changed=src/lib.rs");
}
