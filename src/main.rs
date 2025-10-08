#![no_std]
#![no_main]
#![feature(allocator_api)]
#![allow(unused)]

extern crate alloc;
extern crate riscv_rt;

use core::arch::asm;

use alloc::{string::String, vec::Vec};
use riscv_rt::entry;
mod panic_handler;
mod utils;

mod fs;
mod heap;
mod virtio;

pub const ENTER: u8 = 13;
pub const BACKSPACE: u8 = 127;
pub const CTRL_C: u8 = 3;
pub const CTRL_L: u8 = 12;

pub fn shell() -> ! {
    print!("> ");

    let mut command = String::new();

    loop {
        match sbi::legacy::console_getchar() {
            Some(ENTER) => {
                println!();
                process_command(&command);
                command.clear();
                print!("> ");
            }
            Some(BACKSPACE) => {
                if !command.is_empty() {
                    command.pop();
                    // move left
                    print!("\x08");
                    // clear last character
                    print!(" ");
                    // move cursor back one
                    print!("\x1b[1D");
                }
            }
            Some(CTRL_C) => {
                process_command("exit");
            }
            Some(CTRL_L) => {
                process_command("clear");
            }
            Some(c) => {
                command.push(c as char);
                print!("{}", c as char);
            }
            None => {}
        }
    }
}

fn clear_screen() {
    print!("\x1b[2J\x1b[1;1H");
}

fn print_help_text() {
    println!("available commands:");
    println!("  help      print this help message  (alias: h, ?)");
    println!("  shutdown  shutdown the machine     (alias: sd, exit)");
    println!("  fs        simple filesystem tools   (try: fs ls)");
}

fn process_command(command: &str) {
    match command {
        "help" | "?" | "h" => {
            print_help_text();
        }
        "shutdown" | "sd" | "exit" => utils::shutdown(),
        "clear" => {
            clear_screen();
            print_help_text();
            shell()
        }
        "pagefault" => unsafe {
            core::ptr::read_volatile(0xdeadbeef as *mut u64);
        },
        "breakpoint" => {
            unsafe { asm!("ebreak") };
        }
        c if c.starts_with("fs") => {
            handle_fs_command(c);
        }
        c if c.starts_with("echo") => {
            let output: Vec<_> = c.split_ascii_whitespace().skip(1).collect();
            println!("{}", output.join(" "));
        }
        "" => {}
        _ => {
            println!("unknown command: {command}");
        }
    };
}

fn handle_fs_command(command: &str) {
    let mut parts = command.split_ascii_whitespace();
    let Some(cmd) = parts.next() else {
        return;
    };
    if cmd != "fs" {
        println!("unknown command: {command}");
        return;
    }

    if let Err(err) = crate::fs::init() {
        println!("fs error: {}", err);
        return;
    }

    let Some(subcommand) = parts.next() else {
        print_fs_usage();
        return;
    };

    match subcommand {
        "ls" => match crate::fs::list_files() {
            Ok(files) => {
                if files.is_empty() {
                    println!("(empty)");
                    return;
                }

                for name in files {
                    println!("{}", name);
                }
            }
            Err(err) => println!("fs error: {}", err),
        },
        "cat" => match parts.next() {
            Some(name) => match crate::fs::read_file(name) {
                Ok(contents) => match String::from_utf8(contents) {
                    Ok(text) => println!("{}", text),
                    Err(_) => println!("fs error: file is not valid UTF-8"),
                },
                Err(err) => println!("fs error: {}", err),
            },
            None => {
                println!("usage: fs cat <name>");
            }
        },
        "write" => {
            let trim_fs = command.strip_prefix("fs").unwrap_or("").trim_start();
            let rest = trim_fs.strip_prefix("write").unwrap_or("").trim_start();
            if rest.is_empty() {
                println!("usage: fs write <name> <text>");
                return;
            }
            let Some(name) = rest.split_ascii_whitespace().next() else {
                println!("usage: fs write <name> <text>");
                return;
            };
            let data = rest[name.len()..].trim_start();
            if data.is_empty() {
                println!("usage: fs write <name> <text>");
                return;
            }
            match crate::fs::write_file(name, data.as_bytes()) {
                Ok(()) => println!("wrote {} bytes", data.len()),
                Err(err) => println!("fs error: {}", err),
            }
        }
        "format" => match crate::fs::format() {
            Ok(()) => println!("filesystem formatted"),
            Err(err) => println!("fs error: {}", err),
        },
        _ => {
            print_fs_usage();
        }
    }
}

fn print_fs_usage() {
    println!("fs commands:");
    println!("  fs ls");
    println!("  fs cat <name>");
    println!("  fs write <name> <text>");
    println!("  fs format");
}

#[entry]
fn main(a0: usize) -> ! {
    println!("Hello world from hart {}!\n", a0);

    unsafe {
        heap::init_kernel_heap();
    }

    if let Err(err) = crate::fs::init() {
        println!("failed to initialize filesystem: {}", err);
    }

    shell()
}
