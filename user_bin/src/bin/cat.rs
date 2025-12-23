#![no_std]
#![no_main]

use user_bin::{exit, get_arg, read_file, write};

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let Some(filename) = get_arg(argc, argv, 1) else {
        write(1, b"Usage: cat <file>\n");
        exit(1);
    };

    let mut buf = [0u8; 4096];
    let len = read_file(filename, &mut buf);
    if len <= 0 {
        write(1, b"cat: no data\n");
        exit(1);
    }

    let slice = &buf[..len as usize];
    write(1, slice);
    exit(0)
}
