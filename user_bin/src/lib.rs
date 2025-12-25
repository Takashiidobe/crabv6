#![no_std]

use core::panic::PanicInfo;

// Syscall numbers
pub const SYS_WRITE: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_FILE_WRITE: usize = 3;
pub const SYS_FILE_READ: usize = 4;
pub const SYS_FILE_CREATE: usize = 5;
pub const SYS_FILE_DELETE: usize = 6;
pub const SYS_DIR_CREATE: usize = 7;
pub const SYS_DIR_DELETE: usize = 8;
pub const SYS_OPEN: usize = 9;
pub const SYS_CLOSE: usize = 10;
pub const SYS_READ: usize = 11;
pub const SYS_DUP2: usize = 12;
pub const SYS_PIPE: usize = 13;
pub const SYS_SPAWN: usize = 14;
pub const SYS_WAIT: usize = 15;

// Open flags (bit flags)
pub const O_READ: usize = 0x1;
pub const O_WRITE: usize = 0x2;
pub const O_CREATE: usize = 0x4;
pub const O_APPEND: usize = 0x8;

/// Write data to a file descriptor
pub fn write(fd: usize, buf: &[u8]) -> isize {
    if buf.is_empty() {
        return 0;
    }

    let mut written_total = 0;
    loop {
        let mut ret: isize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a0") SYS_WRITE,
                in("a1") fd,
                in("a2") buf.as_ptr().add(written_total),
                in("a3") buf.len() - written_total,
                lateout("a0") ret,
            );
        }
        if ret == -11 {
            continue;
        }
        if ret < 0 {
            return if written_total > 0 {
                written_total as isize
            } else {
                ret
            };
        }

        let wrote = core::cmp::min(ret as usize, buf.len() - written_total);
        written_total += wrote;

        if written_total >= buf.len() || wrote == 0 {
            return written_total as isize;
        }
    }
}

/// Exit the process with a status code
pub fn exit(code: isize) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_EXIT,
            in("a1") code as usize,
            options(noreturn)
        );
    }
}

/// Read a file into a buffer
pub fn read_file(path: &str, buf: &mut [u8]) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_FILE_READ,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") buf.as_mut_ptr(),
            in("a4") buf.len(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Write data to a file in the filesystem
pub fn write_file(path: &str, data: &[u8]) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_FILE_WRITE,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") data.as_ptr(),
            in("a4") data.len(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Create an empty file
pub fn create_file(path: &str) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_FILE_CREATE,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Delete a file
pub fn delete_file(path: &str) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_FILE_DELETE,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Create a directory
pub fn create_dir(path: &str) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_DIR_CREATE,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Delete an empty directory
pub fn delete_dir(path: &str) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_DIR_DELETE,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Open a file and return a file descriptor
pub fn open(path: &str, flags: usize) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_OPEN,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") flags,
            lateout("a0") ret,
        );
    }
    ret
}

/// Close a file descriptor
pub fn close(fd: usize) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_CLOSE,
            in("a1") fd,
            lateout("a0") ret,
        );
    }
    ret
}

/// Read from a file descriptor
pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    loop {
        let mut ret: isize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a0") SYS_READ,
                in("a1") fd,
                in("a2") buf.as_mut_ptr(),
                in("a3") buf.len(),
                lateout("a0") ret,
            );
        }
        if ret != -11 {
            return ret;
        }
    }
}

/// Duplicate a file descriptor to a specific fd number
pub fn dup2(old_fd: usize, new_fd: usize) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_DUP2,
            in("a1") old_fd,
            in("a2") new_fd,
            lateout("a0") ret,
        );
    }
    ret
}

/// Create a pipe and return read/write file descriptors
/// fds[0] = read end, fds[1] = write end
pub fn pipe(fds: &mut [usize; 2]) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_PIPE,
            in("a1") fds.as_mut_ptr(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Spawn a new process
/// Returns the child PID on success, negative error code on failure
pub fn spawn(path: &str, argv: &[&str]) -> isize {
    // Build argv array of pointers and lengths
    let mut arg_ptrs: [*const u8; 16] = [core::ptr::null(); 16];
    let mut arg_lens: [usize; 16] = [0; 16];
    for (i, &arg) in argv.iter().enumerate() {
        if i >= 16 {
            break;
        }
        arg_ptrs[i] = arg.as_ptr();
        arg_lens[i] = arg.len();
    }

    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_SPAWN,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") arg_ptrs.as_ptr(),
            in("a4") argv.len(),
            in("a5") arg_lens.as_ptr(),
            lateout("a0") ret,
        );
    }
    ret
}

/// Wait for a child process to exit
/// Returns the child PID on success, writes exit code to status if provided
/// Returns negative error code on failure
pub fn wait(status: Option<&mut isize>) -> isize {
    let status_ptr = match status {
        Some(s) => s as *mut isize,
        None => core::ptr::null_mut(),
    };

    // Loop retrying wait() until a child is reaped
    // The syscall returns EAGAIN (-11) when children exist but haven't exited yet
    loop {
        let mut ret: isize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a0") SYS_WAIT,
                in("a1") status_ptr,
                lateout("a0") ret,
            );
        }

        // EAGAIN (-11) means children exist but haven't exited - retry
        // Any other error or success - return immediately
        if ret != -11 {
            return ret;
        }
        // Loop and retry the syscall
        // The kernel has marked us as blocked, so scheduler will run other processes
    }
}

/// Parse command-line arguments and extract argument at index
/// Returns None if index is out of bounds
pub fn get_arg(argc: usize, argv: *const *const u8, index: usize) -> Option<&'static str> {
    write(2, b"[get_arg] called\n");

    if index >= argc {
        write(2, b"[get_arg] index >= argc\n");
        return None;
    }

    #[allow(unused_unsafe)]
    let args = unsafe { core::slice::from_raw_parts(unsafe { argv }, argc) };
    write(2, b"[get_arg] got args slice\n");

    let ptr = args[index];
    write(2, b"[get_arg] got ptr\n");

    let mut len = 0;
    write(2, b"[get_arg] looking for null\n");

    // Check if we can read the first byte
    let first_byte = unsafe { *ptr };
    write(2, b"[get_arg] read first byte\n");

    if first_byte == 0 {
        write(2, b"[get_arg] first byte is null!\n");
        return Some("");
    }

    write(2, b"[get_arg] first byte not null, entering loop\n");

    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
            if len == 10 {
                write(2, b"[get_arg] len=10\n");
            }
            if len == 50 {
                write(2, b"[get_arg] len=50\n");
            }
            if len > 100 {
                write(2, b"[get_arg] len > 100!\n");
                break;
            }
        }
        write(2, b"[get_arg] found null!\n");
        Some(core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            ptr, len,
        )))
    }
}

/// Default panic handler that exits with code 2
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(2)
}
