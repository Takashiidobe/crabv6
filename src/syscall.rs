use alloc::{string::String, vec::Vec};
use core::{fmt::Write, ptr, slice, str};

use riscv::register::sepc;
use riscv_rt::TrapFrame;

use crate::fs::{self, FsError};
use crate::uart;
use crate::proc::PROCESS_TABLE;

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
        SYS_OPEN => sys_open(trap_frame),
        SYS_CLOSE => sys_close(trap_frame),
        SYS_READ => sys_read(trap_frame),
        SYS_DUP2 => sys_dup2(trap_frame),
        SYS_PIPE => sys_pipe(trap_frame),
        SYS_SPAWN => sys_spawn(trap_frame),
        SYS_WAIT => sys_wait(trap_frame),
        _ => Err(SysError::NoSys),
    };

    let code = match result {
        Ok(len) => len as isize,
        Err(SysError::NoSys) => ENOSYS,
        Err(SysError::BadFd) => EBADF,
        Err(SysError::InvalidUtf8) => EINVAL,
        Err(SysError::Fault) => EFAULT,
        Err(SysError::Fs(err)) => fs_errno(err),
        Err(SysError::Fd(err)) => fd_errno(err),
        Err(SysError::Proc(err)) => proc_errno(err),
        Err(SysError::Child) => -10, // ECHILD
        Err(SysError::NoProcess) => EBADF,
    };

    code as usize
}

unsafe fn handle_ecall(trap_frame: &mut TrapFrame) {
    let sepc_value = unsafe { sepc::read().wrapping_add(4) };
    unsafe { sepc::write(sepc_value) };

    let syscall_num = trap_frame.a0;
    let current_pid = crate::proc::PROCESS_TABLE.lock().get_current_pid();

    let retval = dispatch(trap_frame);
    trap_frame.a0 = retval;

    // After syscall, check if we should context switch
    uart::write_str(&alloc::format!("[syscall] pid={} sys={} ret={} calling maybe_switch\n", current_pid, syscall_num, retval as isize));
    crate::scheduler::Scheduler::maybe_switch(trap_frame);
    let after_pid = crate::proc::PROCESS_TABLE.lock().get_current_pid();
    if after_pid != current_pid {
        uart::write_str(&alloc::format!("[syscall] SWITCHED to pid={}\n", after_pid));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SupervisorEnvCall(trap_frame: &mut TrapFrame) {
    unsafe {
        handle_ecall(trap_frame);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn UserEnvCall(trap_frame: &mut TrapFrame) {
    unsafe {
        handle_ecall(trap_frame);
    }
}

fn sys_write(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let fd = trap_frame.a1;
    let ptr = trap_frame.a2 as *const u8;
    let len = trap_frame.a3;

    if len == 0 {
        return Ok(0);
    }

    if ptr.is_null() {
        return Err(SysError::Fault);
    }

    let bytes = unsafe { slice::from_raw_parts(ptr, len) };

    // Capture the writer's PID to use consistently
    let writer_pid = PROCESS_TABLE.lock().get_current_pid();

    loop {
        // Use writer_pid to get the correct process's fd table
        let mut pipe_waiting_on: Option<usize> = None;
        let result = {
            let mut table = PROCESS_TABLE.lock();
            if let Some(proc) = table.get_mut(writer_pid) {
                proc.fd_table
                    .get_mut(fd)
                    .and_then(|fd_entry| {
                        match fd_entry {
                            crate::fd::FileDescriptor::Pipe(pipe_fd) => {
                                pipe_waiting_on = Some(pipe_fd.pipe_id);
                                Ok(pipe_fd.write(bytes))
                            }
                            _ => Ok(fd_entry.write(bytes)),
                        }
                    })
                    .unwrap_or(Err(crate::fd::FdError::BadFd))
            } else {
                Err(crate::fd::FdError::BadFd)
            }
        };

        match result {
            Ok(written) => return Ok(written),
            Err(crate::fd::FdError::WouldBlock) => {
                if let Some(pipe_id) = pipe_waiting_on {
                    let _ = crate::fd::PIPE_TABLE
                        .lock()
                        .mark_writer_waiting(pipe_id, writer_pid);
                }
                crate::scheduler::Scheduler::block_current();
                return Err(SysError::Fd(crate::fd::FdError::WouldBlock));
            }
            Err(e) => return Err(SysError::Fd(e)),
        }
    }
}

fn sys_exit(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let code = trap_frame.a1 as isize;
    {
        let mut table = PROCESS_TABLE.lock();
        let pid = table.get_current_pid();
        if pid != crate::proc::INVALID_PID {
            table.exit_process(pid, code);
            // Unblock any parent waiting for this child
            let parent_pid = table.get(pid).map(|p| p.parent_pid);
            if let Some(parent_pid) = parent_pid {
                if parent_pid != crate::proc::INVALID_PID {
                    crate::scheduler::Scheduler::unblock(parent_pid);
                }
            }
        }
    }
    let mut buf = String::new();
    let _ = writeln!(&mut buf, "\n[process {} exited with code {}]",
        PROCESS_TABLE.lock().get_current_pid(), code);
    uart::write_str(&buf);

    // Process is now Exited, scheduler will switch to another process
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

fn fd_errno(err: crate::fd::FdError) -> isize {
    match err {
        crate::fd::FdError::BadFd => EBADF,
        crate::fd::FdError::TooManyOpen => -24, // EMFILE
        crate::fd::FdError::NotFound => ENOENT,
        crate::fd::FdError::NotImplemented => ENOSYS,
        crate::fd::FdError::WouldBlock => -11, // EAGAIN
        crate::fd::FdError::BrokenPipe => -32, // EPIPE
        crate::fd::FdError::Fs(fs_err) => fs_errno(fs_err),
    }
}

#[derive(Debug, Clone, Copy)]
enum SysError {
    NoSys,
    BadFd,
    InvalidUtf8,
    Fault,
    Fs(FsError),
    Fd(crate::fd::FdError),
    Proc(crate::proc::SpawnError),
    Child, // ECHILD - No child processes
    NoProcess,
}

fn with_current_fd_table_mut<F, R>(f: F) -> Result<R, SysError>
where
    F: FnOnce(&mut crate::fd::FdTable) -> Result<R, crate::fd::FdError>,
{
    let mut table = PROCESS_TABLE.lock();
    let pid = table.get_current_pid();
    if pid == crate::proc::INVALID_PID {
        return Err(SysError::NoProcess);
    }
    let Some(proc) = table.get_mut(pid) else {
        return Err(SysError::NoProcess);
    };
    f(&mut proc.fd_table).map_err(SysError::Fd)
}

fn sys_open(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    let flags = trap_frame.a3;

    // Parse flags: bit 0 = read, bit 1 = write, bit 2 = create, bit 3 = append
    let mode = crate::fd::FileMode {
        read: flags & 0x1 != 0,
        write: flags & 0x2 != 0,
        create: flags & 0x4 != 0,
        append: flags & 0x8 != 0,
    };

    let file_fd = crate::fd::FileFd::open(path, mode).map_err(SysError::Fd)?;
    let fd_num =
        with_current_fd_table_mut(|table| table.alloc(crate::fd::FileDescriptor::File(file_fd)))?;
    Ok(fd_num)
}

fn sys_close(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let fd = trap_frame.a1;
    with_current_fd_table_mut(|table| table.close(fd))?;
    Ok(0)
}

fn sys_read(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let fd = trap_frame.a1;
    let buf_ptr = trap_frame.a2 as *mut u8;
    let buf_len = trap_frame.a3;

    if buf_len > 0 && buf_ptr.is_null() {
        return Err(SysError::Fault);
    }

    let buf = if buf_len == 0 {
        &mut [][..]
    } else {
        unsafe { slice::from_raw_parts_mut(buf_ptr, buf_len) }
    };

    // Capture the reader's PID to use consistently
    let reader_pid = PROCESS_TABLE.lock().get_current_pid();

    // Try to read, block if would block
    loop {
        let mut pipe_waiting_on: Option<usize> = None;
        let result = {
            let mut table = PROCESS_TABLE.lock();
            if let Some(proc) = table.get_mut(reader_pid) {
                proc.fd_table
                    .get_mut(fd)
                    .and_then(|fd_entry| {
                        match fd_entry {
                            crate::fd::FileDescriptor::Pipe(pipe_fd) => {
                                pipe_waiting_on = Some(pipe_fd.pipe_id);
                                Ok(pipe_fd.read(buf))
                            }
                            _ => Ok(fd_entry.read(buf)),
                        }
                    })
                    .unwrap_or(Err(crate::fd::FdError::BadFd))
            } else {
                Err(crate::fd::FdError::BadFd)
            }
        };

        match result {
            Ok(bytes) => return Ok(bytes),
            Err(crate::fd::FdError::WouldBlock) => {
                if let Some(pipe_id) = pipe_waiting_on {
                    let _ = crate::fd::PIPE_TABLE
                        .lock()
                        .mark_reader_waiting(pipe_id, reader_pid);
                }
                crate::scheduler::Scheduler::block_current();
                return Err(SysError::Fd(crate::fd::FdError::WouldBlock));
            }
            Err(e) => return Err(SysError::Fd(e)),
        }
    }
}

fn sys_dup2(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let old_fd = trap_frame.a1;
    let new_fd = trap_frame.a2;

    with_current_fd_table_mut(|table| table.dup2(old_fd, new_fd))?;

    Ok(new_fd)
}

fn sys_pipe(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let fds_ptr = trap_frame.a1 as *mut usize;

    if fds_ptr.is_null() {
        return Err(SysError::Fault);
    }

    // Create a new pipe
    let pipe_id = crate::fd::PipeTable::create_pipe().map_err(SysError::Fd)?;

    // Create file descriptors for both ends
    let read_fd = crate::fd::PipeFd::new(pipe_id, true);
    let write_fd = crate::fd::PipeFd::new(pipe_id, false);

    // Allocate file descriptors
    let mut fd_nums = [0usize; 2];
    with_current_fd_table_mut(|table| {
        let read_fd_num = table.alloc(crate::fd::FileDescriptor::Pipe(read_fd))?;
        let write_fd_num = table
            .alloc(crate::fd::FileDescriptor::Pipe(write_fd))
            .map_err(|e| {
                let _ = table.close(read_fd_num);
                e
            })?;
        fd_nums[0] = read_fd_num;
        fd_nums[1] = write_fd_num;
        Ok(())
    })?;

    // Write the fd numbers to user space
    unsafe {
        ptr::write(fds_ptr, fd_nums[0]);
        ptr::write(fds_ptr.add(1), fd_nums[1]);
    }

    Ok(0)
}

fn sys_spawn(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let path = read_path(trap_frame.a1 as *const u8, trap_frame.a2)?;
    let argv_ptr = trap_frame.a3 as *const *const u8;
    let argc = trap_frame.a4;
    let arg_lens_ptr = trap_frame.a5 as *const usize;

    uart::write_str(&alloc::format!("[spawn] path={}, argc={}\n", path, argc));

    // Parse arguments from user space
    let mut args = alloc::vec![];
    if argc > 0 && !argv_ptr.is_null() {
        for i in 0..argc {
            unsafe {
                let arg_ptr = *argv_ptr.add(i);
                if arg_ptr.is_null() {
                    break;
                }
                // Read the length from the lengths array
                let len = if !arg_lens_ptr.is_null() {
                    *arg_lens_ptr.add(i)
                } else {
                    // Fallback: find string length by searching for null terminator
                    let mut l = 0;
                    while *arg_ptr.add(l) != 0 {
                        l += 1;
                        if l > 4096 {
                            // Prevent infinite loop
                            return Err(SysError::Fault);
                        }
                    }
                    l
                };
                let bytes = slice::from_raw_parts(arg_ptr, len);
                let arg = str::from_utf8(bytes).map_err(|_| SysError::InvalidUtf8)?;
                args.push(String::from(arg));
            }
        }
    }

    let program = crate::process::load(&path).map_err(|_| {
        SysError::Proc(crate::proc::SpawnError::ProgramNotFound)
    })?;

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    // Inherit fds from parent
    let fd_table = {
        let table = PROCESS_TABLE.lock();
        let parent_pid = table.get_current_pid();
        if parent_pid == crate::proc::INVALID_PID {
            crate::fd::FdTable::with_standard()
        } else {
            table
                .get(parent_pid)
                .map(|p| p.fd_table.clone())
                .unwrap_or_else(crate::fd::FdTable::with_standard)
        }
    };

    // Save current user window state
    let mut saved_window = alloc::vec![0u8; crate::process::USER_WINDOW_SIZE];
    crate::process::snapshot_user_window(&mut saved_window);

    // Load child program into user window to build its initial state
    crate::process::load_into_user_window(&program)
        .map_err(|_| SysError::Proc(crate::proc::SpawnError::LoadFailed))?;
    let (sp, built_argc, built_argv_ptr) =
        crate::process::build_user_stack(&arg_refs)
        .map_err(|_| SysError::Proc(crate::proc::SpawnError::LoadFailed))?;

    // Capture child's initial memory state
    let mut child_memory = alloc::vec![0u8; crate::process::USER_WINDOW_SIZE];
    crate::process::snapshot_user_window(&mut child_memory);

    // Restore parent's user window
    crate::process::restore_user_window(&saved_window);

    // Create process entry with child's memory snapshot and initial argc/argv
    let child_pid = {
        let mut table = PROCESS_TABLE.lock();
        let parent_pid = table.get_current_pid();
        uart::write_str(&alloc::format!("[spawn] parent_pid={}, creating child...\n", parent_pid));
        table
            .spawn(program.entry, sp as u64, path.clone(), args.clone(), fd_table, child_memory, built_argc, built_argv_ptr)
            .map_err(SysError::Proc)?
    };

    uart::write_str(&alloc::format!("[spawn] created child_pid={}\n", child_pid));
    uart::write_str("[spawn] returning to parent\n");
    // Child is now Ready - it will run when scheduled
    Ok(child_pid)
}

fn sys_wait(trap_frame: &TrapFrame) -> Result<usize, SysError> {
    let status_ptr = trap_frame.a1 as *mut isize;

    let mut table = PROCESS_TABLE.lock();
    let current_pid = table.get_current_pid();

    if current_pid == crate::proc::INVALID_PID {
        return Err(SysError::Child);
    }

    if !table.has_children(current_pid) {
        return Err(SysError::Child);
    }

    // Try to reap an exited child
    if let Some((child_pid, exit_code)) = table.wait(current_pid) {
        uart::write_str(&alloc::format!("[wait] reaped child_pid={}\n", child_pid));
        if !status_ptr.is_null() {
            unsafe {
                ptr::write(status_ptr, exit_code);
            }
        }
        return Ok(child_pid);
    }

    // No exited children yet - mark as blocked and return EAGAIN
    // The process will be rescheduled when a child exits
    // User-space should retry the syscall
    drop(table);
    crate::scheduler::Scheduler::block_current();

    // Return EAGAIN to indicate "would block"
    Err(SysError::Fd(crate::fd::FdError::WouldBlock))
}

fn proc_errno(err: crate::proc::SpawnError) -> isize {
    match err {
        crate::proc::SpawnError::TooManyProcesses => -24, // EMFILE
        crate::proc::SpawnError::ProgramNotFound => ENOENT,
        crate::proc::SpawnError::LoadFailed => EIO,
        crate::proc::SpawnError::OutOfMemory => -12, // ENOMEM
    }
}
