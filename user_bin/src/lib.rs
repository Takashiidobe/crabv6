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

/// Write data to a file descriptor
pub fn write(fd: usize, buf: &[u8]) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_WRITE,
            in("a1") fd,
            in("a2") buf.as_ptr(),
            in("a3") buf.len(),
            lateout("a0") ret,
        );
    }
    ret
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

/// Parse command-line arguments and extract argument at index
/// Returns None if index is out of bounds
pub fn get_arg(argc: usize, argv: *const *const u8, index: usize) -> Option<&'static str> {
    if index >= argc {
        return None;
    }

    #[allow(unused_unsafe)]
    let args = unsafe { core::slice::from_raw_parts(unsafe { argv }, argc) };
    let ptr = args[index];
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
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
