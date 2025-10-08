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

mod heap;

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

#[entry]
fn main(a0: usize) -> ! {
    println!("Hello world from hart {}!\n", a0);

    unsafe {
        heap::init_kernel_heap();
    }

    shell()
}
