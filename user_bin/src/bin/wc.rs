#![no_std]
#![no_main]

use user_bin::{exit, get_arg, read_file, write};

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let Some(filename) = get_arg(argc, argv, 1) else {
        write(1, b"Usage: wc <file>\n");
        exit(1);
    };

    let mut buf = [0u8; 4096];
    let len = read_file(filename, &mut buf);
    if len < 0 {
        write(1, b"wc: error reading file\n");
        exit(1);
    }

    let data = &buf[..len as usize];

    // Count lines, words, and bytes
    let lines = count_lines(data);
    let words = count_words(data);
    let bytes = len as usize;

    // Format and print output
    print_number(lines);
    write(1, b" ");
    print_number(words);
    write(1, b" ");
    print_number(bytes);
    write(1, b" ");
    write(1, filename.as_bytes());
    write(1, b"\n");

    exit(0)
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
