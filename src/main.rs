#![no_std]
#![no_main]
#![feature(allocator_api)]
#![allow(unused)]

extern crate alloc;

use core::arch::asm;

use crate::process::LoadError;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use riscv_rt::entry;
mod panic_handler;
mod utils;

mod elf;
mod embedded;
mod fd;
mod fs;
mod heap;
mod interrupts;
mod proc;
mod process;
mod scheduler;
mod syscall;
mod uart;
mod user;
mod virtio;

core::arch::global_asm!(include_str!("kernel_entry.S"));

pub const ENTER: u8 = 13;
pub const BACKSPACE: u8 = 127;
pub const CTRL_C: u8 = 3;
pub const CTRL_L: u8 = 12;

fn clear_screen() {
    print!("\x1b[2J\x1b[1;1H");
}

fn print_help_text() {
    println!("available commands:");
    println!("  help      print this help message  (alias: h, ?)");
    println!("  shutdown  shutdown the machine     (alias: sd, exit)");
    println!("  echo      print text to console");
    println!("  ls        list directory contents  (usage: ls [path])");
    println!("  cd        change directory         (usage: cd <path>)");
    println!("  fs        simple filesystem tools  (try: fs ls)");
    println!("  run       load and execute ELF user program");
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
        c if c.starts_with("run") => {
            handle_run_command(c, cwd);
        }
        "syscalltest" => unsafe {
            let msg = b"hello from syscall\n";
            let mut ret: usize;
            asm!(
                "ecall",
                in("a0") crate::syscall::SYS_WRITE,
                in("a1") 1usize,
                in("a2") msg.as_ptr(),
                in("a3") msg.len(),
                lateout("a0") ret,
            );
            println!("sys_write returned {}", ret as isize);
        },
        command if command.starts_with("fs") => {
            handle_fs_command(command, cwd);
        }
        command if command.starts_with("echo") => {
            let output: Vec<_> = command.split_ascii_whitespace().skip(1).collect();
            println!("{}", output.join(" "));
        }
        command if command.starts_with("ls") => {
            let mut parts = command.split_ascii_whitespace();
            parts.next(); // Skip "ls"
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

            if let Err(err) = crate::fs::init() {
                println!("fs error: {}", err);
                return;
            }

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
        command if command.starts_with("cd") => {
            let mut parts = command.split_ascii_whitespace();
            parts.next(); // Skip "cd"
            let path_arg = parts.next().unwrap_or("/");
            let target = normalize_path(cwd.as_str(), path_arg);
            let fs_path = if target.is_empty() {
                ""
            } else {
                target.as_str()
            };

            if let Err(err) = crate::fs::init() {
                println!("fs error: {}", err);
                return;
            }

            match crate::fs::ensure_directory(fs_path) {
                Ok(()) => {
                    *cwd = target;
                }
                Err(err) => println!("fs error: {}", err),
            }
        }
        "" => {}
        _ => {
            // Defer complex shell features to user-space /bin/sh
            if command.contains(['|', '>', '<']) {
                println!("Pipes/redirection are handled in /bin/sh. Launch the user shell to run: {command}");
                return;
            }

            // Try to execute as a binary in /bin/
            let first_word = command.split_ascii_whitespace().next().unwrap_or("");
            if !first_word.is_empty() {
                let bin_path = alloc::format!("/bin/{}", first_word);

                // Check if binary exists
                if let Err(err) = crate::fs::init() {
                    println!("fs error: {}", err);
                    return;
                }

                match crate::fs::read_file(&bin_path) {
                    Ok(_) => {
                        // Binary exists, execute it with full path
                        let rest_of_command: Vec<&str> =
                            command.split_ascii_whitespace().skip(1).collect();
                        let run_command = if rest_of_command.is_empty() {
                            alloc::format!("run {}", bin_path)
                        } else {
                            alloc::format!("run {} {}", bin_path, rest_of_command.join(" "))
                        };
                        handle_run_command(&run_command, cwd);
                    }
                    Err(_) => {
                        println!("unknown command: {command}");
                    }
                }
            }
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
        "rm" => {
            if let Some(path) = parts.next() {
                let target = normalize_path(cwd.as_str(), path);
                let fs_path = if target.is_empty() {
                    ""
                } else {
                    target.as_str()
                };
                match crate::fs::remove_file(fs_path) {
                    Ok(()) => println!("created directory {}", path),
                    Err(err) => println!("fs error: {}", err),
                }
            } else {
                println!("usage: fs rm <path>");
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
    println!("  fs cat <path>");
    println!("  fs write <path> <text>");
    println!("  fs rm <path>");
    println!("  fs mkdir <path>");
    println!("  fs format");
}

fn handle_run_command(command: &str, cwd: &str) {
    let mut parts = command.split_ascii_whitespace();
    let Some(cmd) = parts.next() else {
        return;
    };
    if cmd != "run" {
        println!("unknown command: {command}");
        return;
    }

    // Path to the binary to run
    let Some(path_arg) = parts.next() else {
        println!("usage: run <path> [args...]");
        return;
    };

    let extra_args: Vec<&str> = parts.collect();

    if let Err(err) = crate::fs::init() {
        println!("fs error: {}", err);
        return;
    }

    let target = normalize_path(cwd, path_arg);
    let path = target.as_str();

    match crate::process::load(path) {
        Ok(program) => {
            crate::process::dump(&program);
            println!("launching {}", path);

            // Normalize all arguments relative to current working directory
            // This ensures that paths like "test.txt" work correctly
            let normalized_args: Vec<String> = extra_args
                .iter()
                .map(|&arg| normalize_path(cwd, arg))
                .collect();

            let mut args: Vec<&str> = Vec::new();
            args.push(path);
            for arg in &normalized_args {
                args.push(arg.as_str());
            }

            unsafe { crate::process::enter_user(&program, &args) };
        }
        Err(LoadError::Fs(err)) => println!("fs error: {}", err),
        Err(LoadError::Elf(err)) => println!("elf error: {:?}", err),
        Err(LoadError::OutOfMemory) => println!("loader error: out of memory"),
    }
}

fn handle_pipe(_command: &str, _cwd: &str) {}

fn handle_output_redirect(command: &str, cwd: &str) {
    let Some(redir_pos) = command.find('>') else {
        return;
    };

    let cmd_part = command[..redir_pos].trim();
    let file_part = command[redir_pos + 1..].trim();

    // Check for append mode (>>)
    let (file_path, mode) = if file_part.starts_with('>') {
        let file = file_part[1..].trim();
        (normalize_path(cwd, file), crate::fd::FileMode {
            read: false,
            write: true,
            create: true,
            append: true,
        })
    } else {
        (normalize_path(cwd, file_part), crate::fd::FileMode {
            read: false,
            write: true,
            create: true,
            append: false,
        })
    };

    // Open output file
    let file_fd = match crate::fd::FileFd::open(file_path.clone(), mode) {
        Ok(file_fd) => file_fd,
        Err(err) => {
            println!("Failed to open {} for writing: {:?}", file_path, err);
            return;
        }
    };

    let fd = match crate::fd::FD_TABLE.lock().alloc(crate::fd::FileDescriptor::File(file_fd)) {
        Ok(fd) => fd,
        Err(err) => {
            println!("Failed to allocate fd: {:?}", err);
            return;
        }
    };

    // Save stdout
    let saved_stdout = match crate::fd::FD_TABLE.lock().dup2(1, 10) {
        Ok(_) => 10,
        Err(err) => {
            println!("Failed to save stdout: {:?}", err);
            let _ = crate::fd::FD_TABLE.lock().close(fd);
            return;
        }
    };

    // Redirect stdout to file
    let _ = crate::fd::FD_TABLE.lock().dup2(fd, 1);
    let _ = crate::fd::FD_TABLE.lock().close(fd);

    // Execute command
    execute_simple_command(cmd_part, cwd);

    // Restore stdout
    let _ = crate::fd::FD_TABLE.lock().dup2(saved_stdout, 1);
    let _ = crate::fd::FD_TABLE.lock().close(saved_stdout);
}

fn handle_input_redirect(command: &str, cwd: &str) {
    let Some(redir_pos) = command.find('<') else {
        return;
    };

    let cmd_part = command[..redir_pos].trim();
    let file_part = command[redir_pos + 1..].trim();
    let file_path = normalize_path(cwd, file_part);

    // Open input file
    let mode = crate::fd::FileMode {
        read: true,
        write: false,
        create: false,
        append: false,
    };

    let file_fd = match crate::fd::FileFd::open(file_path.clone(), mode) {
        Ok(file_fd) => file_fd,
        Err(err) => {
            println!("Failed to open {} for reading: {:?}", file_path, err);
            return;
        }
    };

    let fd = match crate::fd::FD_TABLE.lock().alloc(crate::fd::FileDescriptor::File(file_fd)) {
        Ok(fd) => fd,
        Err(err) => {
            println!("Failed to allocate fd: {:?}", err);
            return;
        }
    };

    // Save stdin
    let saved_stdin = match crate::fd::FD_TABLE.lock().dup2(0, 10) {
        Ok(_) => 10,
        Err(err) => {
            println!("Failed to save stdin: {:?}", err);
            let _ = crate::fd::FD_TABLE.lock().close(fd);
            return;
        }
    };

    // Redirect stdin from file
    let _ = crate::fd::FD_TABLE.lock().dup2(fd, 0);
    let _ = crate::fd::FD_TABLE.lock().close(fd);

    // Execute command
    execute_simple_command(cmd_part, cwd);

    // Restore stdin
    let _ = crate::fd::FD_TABLE.lock().dup2(saved_stdin, 0);
    let _ = crate::fd::FD_TABLE.lock().close(saved_stdin);
}

fn execute_simple_command(command: &str, cwd: &str) {
    let first_word = command.split_ascii_whitespace().next().unwrap_or("");
    if first_word.is_empty() {
        return;
    }

    let bin_path = alloc::format!("/bin/{}", first_word);
    let rest_of_command: Vec<&str> = command.split_ascii_whitespace().skip(1).collect();
    let run_command = if rest_of_command.is_empty() {
        alloc::format!("run {}", bin_path)
    } else {
        alloc::format!("run {} {}", bin_path, rest_of_command.join(" "))
    };
    handle_run_command(&run_command, cwd);
}

fn print_prompt(cwd: &str) {
    if cwd.is_empty() {
        print!("/> ");
    } else {
        print!("{}/> ", cwd);
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

    if segments.is_empty() {
        String::new()
    } else {
        alloc::format!("/{}", segments.join("/"))
    }
}

fn install_embedded_bins() {
    use crate::fs::{self, FsError};

    if let Err(err) = fs::mkdir("/bin")
        && !matches!(err, FsError::AlreadyExists)
    {
        println!("fs error: {}", err);
        return;
    }

    if let Err(err) = fs::write_file("test.txt", "hello world".as_bytes()) {
        println!("fs error: {}", err);
        return;
    }

    match fs::read_file("/bin/cat") {
        Ok(_) => {}
        Err(FsError::NotFound) => match fs::write_file("/bin/cat", crate::embedded::CAT_BIN) {
            Ok(_) => println!("installed /bin/cat"),
            Err(err) => println!("fs error: {}", err),
        },
        Err(err) => println!("fs error: {}", err),
    }

    match fs::read_file("/bin/sh") {
        Ok(_) => {}
        Err(FsError::NotFound) => match fs::write_file("/bin/sh", crate::embedded::SH_BIN) {
            Ok(_) => println!("installed /bin/sh"),
            Err(err) => println!("fs error: {}", err),
        },
        Err(err) => println!("fs error: {}", err),
    }

    match fs::read_file("/bin/wc") {
        Ok(_) => {}
        Err(FsError::NotFound) => match fs::write_file("/bin/wc", crate::embedded::WC_BIN) {
            Ok(_) => println!("installed /bin/wc"),
            Err(err) => println!("fs error: {}", err),
        },
        Err(err) => println!("fs error: {}", err),
    }
}

fn launch_user_shell() -> ! {
    let sh_path = "/bin/sh";
    let args = [sh_path];

    let program = match crate::process::load(sh_path) {
        Ok(p) => p,
        Err(_) => {
            println!("failed to load /bin/sh");
            return idle_loop();
        }
    };

    // Load shell into user window and build its stack
    if let Err(_) = crate::process::load_into_user_window(&program) {
        println!("failed to load shell image");
        return idle_loop();
    }
    let (sp, _argc, _argv_ptr) = match crate::process::build_user_stack(&args) {
        Ok(v) => v,
        Err(_) => {
            println!("failed to build shell stack");
            return idle_loop();
        }
    };

    let (sp, shell_argc, shell_argv_ptr) = match crate::process::build_user_stack(&args) {
        Ok(v) => v,
        Err(_) => {
            println!("failed to build shell stack");
            return idle_loop();
        }
    };

    // Capture shell's initial memory state
    let mut shell_memory = alloc::vec![0u8; crate::process::USER_WINDOW_SIZE];
    crate::process::snapshot_user_window(&mut shell_memory);

    // Create shell process with its memory snapshot
    {
        let mut table = crate::proc::PROCESS_TABLE.lock();
        let pid = match table.spawn(
            program.entry,
            sp as u64,
            sh_path.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
            fd::FdTable::with_standard(),
            shell_memory,
            shell_argc,
            shell_argv_ptr,
        ) {
            Ok(pid) => pid,
            Err(_) => {
                println!("failed to spawn shell");
                return idle_loop();
            }
        };
        // Don't set as current yet - scheduler will handle it
    }

    println!("Shell process created, starting execution...");

    // Restore shell's memory and enter it
    // After this, all scheduling happens via trap handlers
    let (entry, sp) = {
        let mut table = crate::proc::PROCESS_TABLE.lock();
        let shell_pid = table.get_all_processes().first().expect("no shell process").pid;
        table.set_current(shell_pid);
        table.restore_process_memory(shell_pid);

        let process = table.get(shell_pid).expect("shell process vanished");
        (process.pc, process.sp)
    };

    // Enter user mode for shell
    // Scheduling happens via syscall trap handlers calling Scheduler::maybe_switch
    // This never returns in normal operation
    unsafe { crate::process::enter_user_at(entry, sp, 0, 0) };

    println!("All processes exited");
    idle_loop()
}

#[entry]
fn main(a0: usize) -> ! {
    if a0 != 0 {
        idle_loop();
    }

    unsafe {
        heap::init_kernel_heap();
    }

    uart::init();
    interrupts::init();

    println!("Hello world from hart {}!\n", a0);

    match crate::fs::init() {
        Ok(()) => install_embedded_bins(),
        Err(err) => println!("failed to initialize filesystem: {}", err),
    }

    launch_user_shell()
}

fn idle_loop() -> ! {
    loop {
        unsafe {
            riscv::asm::wfi();
        }
    }
}
