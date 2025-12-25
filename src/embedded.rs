use crate::println;

pub const CAT_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cat.bin"));
pub const WC_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wc.bin"));
pub const SH_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sh.bin"));

pub fn install_embedded_bins() {
    println!("Installing embedded binaries...");

    // Install cat
    if let Err(err) = crate::fs::write_file("/bin/cat", CAT_BIN) {
        println!("Failed to install /bin/cat: {}", err);
    }

    // Install wc
    if let Err(err) = crate::fs::write_file("/bin/wc", WC_BIN) {
        println!("Failed to install /bin/wc: {}", err);
    }

    // Install sh
    if let Err(err) = crate::fs::write_file("/bin/sh", SH_BIN) {
        println!("Failed to install /bin/sh: {}", err);
    }

    println!("Installed embedded binaries: cat, wc, sh");
}
