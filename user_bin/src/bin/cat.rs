#![no_std]
#![no_main]

use user_bin::{close, exit, get_arg, open, read, write, O_READ};

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    write(2, b"[cat] _start called\n");

    // If no arguments, read from stdin
    if argc == 1 {
        write(2, b"[cat] argc=1, reading stdin\n");
        cat_fd(0);
        exit(0);
    }

    write(2, b"[cat] argc>1, opening files\n");

    // Otherwise, cat each file argument
    let mut i = 1;
    write(2, b"[cat] entering while loop\n");
    while i < argc {
        write(2, b"[cat] calling get_arg\n");
        let Some(filename) = get_arg(argc, argv, i) else {
            write(2, b"[cat] get_arg returned None\n");
            break;
        };
        write(2, b"[cat] get_arg returned\n");

        write(2, b"[cat] opening file: ");
        write(2, filename.as_bytes());
        write(2, b"\n");

        let fd = open(filename, O_READ);

        if fd < 0 {
            write(2, b"[cat] open failed\n");
            write(2, b"cat: cannot open ");
            write(2, filename.as_bytes());
            write(2, b"\n");
            exit(1);
        }

        write(2, b"[cat] reading from file\n");

        cat_fd(fd as usize);
        close(fd as usize);
        i += 1;
    }

    exit(0)
}

fn cat_fd(fd: usize) {
    let mut buf = [0u8; 4096];
    loop {
        let len = read(fd, &mut buf);
        if len <= 0 {
            break;
        }
        write(1, &buf[..len as usize]);
    }
}
