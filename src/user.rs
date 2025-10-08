use core::arch::asm;

use crate::syscall::{
    SYS_DIR_CREATE, SYS_DIR_DELETE, SYS_EXIT, SYS_FILE_CREATE, SYS_FILE_DELETE, SYS_FILE_READ,
    SYS_FILE_WRITE, SYS_WRITE,
};

#[inline]
pub fn write(fd: usize, buf: &[u8]) -> isize {
    let mut ret: isize;
    unsafe {
        asm!(
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

#[inline]
pub fn exit(code: isize) -> ! {
    unsafe {
        asm!(
            "ecall",
            in("a0") SYS_EXIT,
            in("a1") code as usize,
        );
    }
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) }
    }
}

#[inline]
pub fn write_file(path: &str, data: &[u8]) -> isize {
    let mut ret: isize;
    unsafe {
        asm!(
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

#[inline]
pub fn read_file(path: &str, buf: &mut [u8]) -> isize {
    let mut ret: isize;
    unsafe {
        asm!(
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

#[inline]
pub fn create_file(path: &str) -> isize {
    syscall_path_only(SYS_FILE_CREATE, path)
}

#[inline]
pub fn remove_file(path: &str) -> isize {
    syscall_path_only(SYS_FILE_DELETE, path)
}

#[inline]
pub fn create_dir(path: &str) -> isize {
    syscall_path_only(SYS_DIR_CREATE, path)
}

#[inline]
pub fn remove_dir(path: &str) -> isize {
    syscall_path_only(SYS_DIR_DELETE, path)
}

#[inline]
fn syscall_path_only(sysno: usize, path: &str) -> isize {
    let mut ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") sysno,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}
