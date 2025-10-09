use alloc::string::String;
use core::{fmt::Write, ptr, slice, str};

use riscv::register::sepc;
use riscv_rt::TrapFrame;

use crate::fs::{self, FsError};
use crate::uart;

pub const SYS_WRITE: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_FILE_WRITE: usize = 3;
pub const SYS_FILE_READ: usize = 4;
pub const SYS_FILE_CREATE: usize = 5;
pub const SYS_FILE_DELETE: usize = 6;
pub const SYS_DIR_CREATE: usize = 7;
pub const SYS_DIR_DELETE: usize = 8;

const ENOSYS: isize = -38;
const EBADF: isize = -9;
const EINVAL: isize = -22;
const EFAULT: isize = -14;
const ENOENT: isize = -2;
const ENOTDIR: isize = -20;
const EEXIST: isize = -17;
const ENOSPC: isize = -28;
const EISDIR: isize = -21;
const ENOTEMPTY: isize = -39;
const EIO: isize = -5;
const ENXIO: isize = -6;
const ENAMETOOLONG: isize = -36;

pub fn dispatch(trap_frame: &TrapFrame) -> usize {
    let syscall_no = trap_frame.a0;
    let result = match syscall_no {
        SYS_WRITE => sys_write(trap_frame),
        SYS_EXIT => sys_exit(trap_frame),
        SYS_FILE_WRITE => sys_file_write(trap_frame),
        SYS_FILE_READ => sys_file_read(trap_frame),
        SYS_FILE_CREATE => sys_file_create(trap_frame),
        SYS_FILE_DELETE => sys_file_delete(trap_frame),
        SYS_DIR_CREATE => sys_dir_create(trap_frame),
        SYS_DIR_DELETE => sys_dir_delete(trap_frame),
        _ => Err(SysError::NoSys),
    };

    let code = match result {
        Ok(len) => len as isize,
        Err(SysError::NoSys) => ENOSYS,
        Err(SysError::BadFd) => EBADF,
        Err(SysError::InvalidUtf8) => EINVAL,
        Err(SysError::Fault) => EFAULT,
        Err(SysError::Fs(err)) => fs_errno(err),
    };

    code as usize
}

unsafe fn handle_ecall(trap_frame: &TrapFrame) {
    let trap_ptr = trap_frame as *const TrapFrame as *mut TrapFrame;
    let sepc_value = unsafe { sepc::read().wrapping_add(4) };
    unsafe { sepc::write(sepc_value) };
    let retval = dispatch(trap_frame);
    unsafe { ptr::write(&mut (*trap_ptr).a0, retval) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SupervisorEnvCall(trap_frame: &TrapFrame) {
    unsafe {
        handle_ecall(trap_frame);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn UserEnvCall(trap_frame: &TrapFrame) {
    unsafe {
        handle_ecall(trap_frame);
    }
}

fn sys_write(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let fd = trap_frame.a1;
    let ptr = trap_frame.a2 as *const u8;
    let len = trap_frame.a3;

    if fd != 1 && fd != 2 {
        return Err(SysError::BadFd);
    }

    if len == 0 {
        return Ok(0);
    }

    if ptr.is_null() {
        return Err(SysError::Fault);
    }

    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    uart::write_bytes(bytes);
    Ok(len)
}

fn sys_exit(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let code = trap_frame.a1 as isize;
    let mut buf = String::new();
    let _ = writeln!(&mut buf, "[process exited with code {}]", code);
    uart::write_str(&buf);
    let trap_ptr = trap_frame as *const TrapFrame as *mut TrapFrame;
    unsafe {
        crate::process::prepare_for_kernel_return(trap_ptr, code);
    }
    Ok(code as usize)
}

fn sys_file_write(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    let data_ptr = trap_frame.a3 as *const u8;
    let data_len = trap_frame.a4;

    let data = if data_len == 0 {
        &[]
    } else {
        if data_ptr.is_null() {
            return Err(SysError::Fault);
        }
        unsafe { slice::from_raw_parts(data_ptr, data_len) }
    };

    fs::write_file(&path, data).map_err(SysError::Fs)?;
    Ok(data_len)
}

fn sys_file_read(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    let buf_ptr = trap_frame.a3 as *mut u8;
    let buf_len = trap_frame.a4;

    if buf_len > 0 && buf_ptr.is_null() {
        return Err(SysError::Fault);
    }

    let contents = fs::read_file(&path).map_err(SysError::Fs)?;
    let to_copy = contents.len().min(buf_len);
    if to_copy > 0 {
        unsafe { ptr::copy_nonoverlapping(contents.as_ptr(), buf_ptr, to_copy) };
    }
    Ok(to_copy)
}

fn sys_file_create(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    fs::create_file(&path).map_err(SysError::Fs)?;
    Ok(0)
}

fn sys_file_delete(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    fs::remove_file(&path).map_err(SysError::Fs)?;
    Ok(0)
}

fn sys_dir_create(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    fs::mkdir(&path).map_err(SysError::Fs)?;
    Ok(0)
}

fn sys_dir_delete(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    fs::remove_directory(&path).map_err(SysError::Fs)?;
    Ok(0)
}

fn read_path(ptr: *const u8, len: usize) -> Result<String, SysError> {
    if len == 0 {
        return Ok(String::new());
    }
    if ptr.is_null() {
        return Err(SysError::Fault);
    }
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    let s = str::from_utf8(bytes).map_err(|_| SysError::InvalidUtf8)?;
    Ok(String::from(s))
}

fn fs_errno(err: FsError) -> isize {
    match err {
        FsError::NotInitialized => EIO,
        FsError::NameTooLong => ENAMETOOLONG,
        FsError::DirectoryFull | FsError::NoSpace => ENOSPC,
        FsError::NotFound => ENOENT,
        FsError::InvalidEncoding | FsError::InvalidPath => EINVAL,
        FsError::DeviceInitFailed(_) => ENXIO,
        FsError::NotADirectory | FsError::IsFile => ENOTDIR,
        FsError::AlreadyExists => EEXIST,
        FsError::DirectoryNotEmpty => ENOTEMPTY,
        FsError::IsDirectory => EISDIR,
    }
}

#[derive(Debug, Clone, Copy)]
enum SysError {
    NoSys,
    BadFd,
    InvalidUtf8,
    Fault,
    Fs(FsError),
}
