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
    let mut cwd = String::new();
    print_prompt(&cwd);
    let mut command = String::new();

    loop {
        match sbi::legacy::console_getchar() {
            Some(ENTER) => {
                println!();
                process_command(&command, &mut cwd);
                command.clear();
                print_prompt(&cwd);
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
                process_command("exit", &mut cwd);
            }
            Some(CTRL_L) => {
                process_command("clear", &mut cwd);
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

fn process_command(command: &str, cwd: &mut String) {
    match command {
        "help" | "?" | "h" => {
            print_help_text();
        }
        "shutdown" | "sd" | "exit" => utils::shutdown(),
        "clear" => {
            clear_screen();
            print_help_text();
        }
        "pagefault" => unsafe {
            core::ptr::read_volatile(0xdeadbeef as *mut u64);
        },
        "breakpoint" => {
            unsafe { asm!("ebreak") };
        }
        c if c.starts_with("fs") => {
            handle_fs_command(c, cwd);
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

fn handle_fs_command(command: &str, cwd: &mut String) {
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
        "ls" => {
            let target_path = if let Some(arg) = parts.next() {
                normalize_path(cwd.as_str(), arg)
            } else {
                cwd.clone()
            };
            let path_opt = if target_path.is_empty() {
                None
            } else {
                Some(target_path.as_str())
            };
            match crate::fs::list_files(path_opt) {
                Ok(entries) => {
                    if entries.is_empty() {
                        println!("(empty)");
                    } else {
                        for name in entries {
                            println!("{}", name);
                        }
                    }
                }
                Err(err) => println!("fs error: {}", err),
            }
        }
        "cd" => {
            let path_arg = parts.next().unwrap_or("/");
            let target = normalize_path(cwd.as_str(), path_arg);
            let fs_path = if target.is_empty() {
                ""
            } else {
                target.as_str()
            };
            match crate::fs::ensure_directory(fs_path) {
                Ok(()) => {
                    *cwd = target;
                }
                Err(err) => println!("fs error: {}", err),
            }
        }
        "mkdir" => {
            if let Some(path) = parts.next() {
                let target = normalize_path(cwd.as_str(), path);
                let fs_path = if target.is_empty() {
                    ""
                } else {
                    target.as_str()
                };
                match crate::fs::mkdir(fs_path) {
                    Ok(()) => println!("created directory {}", path),
                    Err(err) => println!("fs error: {}", err),
                }
            } else {
                println!("usage: fs mkdir <path>");
            }
        }
        "cat" => {
            if let Some(path) = parts.next() {
                let target = normalize_path(cwd.as_str(), path);
                let fs_path = if target.is_empty() {
                    ""
                } else {
                    target.as_str()
                };
                match crate::fs::read_file(fs_path) {
                    Ok(contents) => match String::from_utf8(contents) {
                        Ok(text) => println!("{}", text),
                        Err(_) => println!("fs error: file is not valid UTF-8"),
                    },
                    Err(err) => println!("fs error: {}", err),
                }
            } else {
                println!("usage: fs cat <path>");
            }
        }
        "write" => {
            let rest = command.strip_prefix("fs").unwrap_or("").trim_start();
            let rest = rest.strip_prefix("write").unwrap_or("").trim_start();
            if rest.is_empty() {
                println!("usage: fs write <path> <text>");
                return;
            }
            let mut rest_parts = rest.splitn(2, |c: char| c.is_ascii_whitespace());
            let Some(path) = rest_parts.next() else {
                println!("usage: fs write <path> <text>");
                return;
            };
            let Some(data) = rest_parts.next() else {
                println!("usage: fs write <path> <text>");
                return;
            };
            let target = normalize_path(cwd.as_str(), path);
            let fs_path = if target.is_empty() {
                ""
            } else {
                target.as_str()
            };
            match crate::fs::write_file(fs_path, data.as_bytes()) {
                Ok(()) => println!("wrote {} bytes", data.len()),
                Err(err) => println!("fs error: {}", err),
            }
        }
        "format" => match crate::fs::format() {
            Ok(()) => {
                *cwd = String::new();
                println!("filesystem formatted");
            }
            Err(err) => println!("fs error: {}", err),
        },
        _ => {
            print_fs_usage();
        }
    }
}

fn print_fs_usage() {
    println!("fs commands:");
    println!("  fs ls [path]");
    println!("  fs cat <path>");
    println!("  fs write <path> <text>");
    println!("  fs cd <path>");
    println!("  fs mkdir <path>");
    println!("  fs format");
}

fn print_prompt(cwd: &str) {
    if cwd.is_empty() {
        print!("/> ");
    } else {
        print!("/{}/> ", cwd);
    }
}

fn normalize_path(cwd: &str, input: &str) -> String {
    if input.is_empty() {
        return String::from(cwd);
    }

    let mut segments: Vec<String> = if input.starts_with('/') {
        Vec::new()
    } else {
        cwd.split('/')
            .filter(|segment| !segment.is_empty())
            .map(String::from)
            .collect()
    };

    for part in input.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            segments.pop();
            continue;
        }
        segments.push(String::from(part));
    }

    segments.join("/")
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
