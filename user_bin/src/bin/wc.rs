#![no_std]
#![no_main]

use user_bin::{close, exit, get_arg, open, read, write, O_READ};

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    // If no arguments, read from stdin
    if argc == 1 {
        wc_fd(0, None);
        exit(0);
    }

    // Otherwise, wc each file argument
    let mut total_lines = 0;
    let mut total_words = 0;
    let mut total_bytes = 0;
    let mut file_count = 0;

    let mut i = 1;
    while i < argc {
        let Some(filename) = get_arg(argc, argv, i) else {
            break;
        };

        let fd = open(filename, O_READ);
        if fd < 0 {
            write(2, b"wc: cannot open ");
            write(2, filename.as_bytes());
            write(2, b"\n");
            exit(1);
        }

        let (lines, words, bytes) = wc_fd(fd as usize, Some(filename));
        close(fd as usize);

        total_lines += lines;
        total_words += words;
        total_bytes += bytes;
        file_count += 1;
        i += 1;
    }

    // If multiple files, print totals
    if file_count > 1 {
        print_number(total_lines);
        write(1, b" ");
        print_number(total_words);
        write(1, b" ");
        print_number(total_bytes);
        write(1, b" total\n");
    }

    exit(0)
}

fn wc_fd(fd: usize, filename: Option<&str>) -> (usize, usize, usize) {
    let mut buf = [0u8; 4096];
    let mut total_bytes = 0;
    let mut lines = 0;
    let mut words = 0;
    let mut in_word = false;

    loop {
        let len = read(fd, &mut buf);
        if len <= 0 {
            break;
        }

        let data = &buf[..len as usize];
        total_bytes += data.len();

        // Count lines and words
        for &byte in data {
            if byte == b'\n' {
                lines += 1;
            }

            let is_whitespace = byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r';
            if !is_whitespace && !in_word {
                words += 1;
                in_word = true;
            } else if is_whitespace {
                in_word = false;
            }
        }
    }

    // Print results
    print_number(lines);
    write(1, b" ");
    print_number(words);
    write(1, b" ");
    print_number(total_bytes);

    if let Some(name) = filename {
        write(1, b" ");
        write(1, name.as_bytes());
    }
    write(1, b"\n");

    (lines, words, total_bytes)
}

fn count_lines(data: &[u8]) -> usize {
    let mut count = 0;
    for &byte in data {
        if byte == b'\n' {
            count += 1;
        }
    }
    count
}

fn count_words(data: &[u8]) -> usize {
    let mut count = 0;
    let mut in_word = false;

    for &byte in data {
        let is_whitespace = byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r';

        if !is_whitespace && !in_word {
            count += 1;
            in_word = true;
        } else if is_whitespace {
            in_word = false;
        }
    }

    count
}

fn print_number(mut num: usize) {
    if num == 0 {
        write(1, b"0");
        return;
    }

    // Convert number to string (max 10 digits for usize on 32-bit)
    let mut buf = [0u8; 20];
    let mut i = 0;

    while num > 0 {
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }

    // Reverse and print
    while i > 0 {
        i -= 1;
        write(1, &buf[i..i+1]);
    }
}
