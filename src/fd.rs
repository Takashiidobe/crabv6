use alloc::{string::String, vec::Vec};
use core::fmt;
use spin::Mutex;

use crate::fs;
use crate::proc::Pid;
use crate::scheduler::Scheduler;

/// Maximum number of open file descriptors
pub const MAX_FDS: usize = 16;

/// Standard file descriptor numbers
pub const STDIN_FD: usize = 0;
pub const STDOUT_FD: usize = 1;
pub const STDERR_FD: usize = 2;

/// Global file descriptor table for kernel-side helpers (kernel shell)
pub static FD_TABLE: Mutex<FdTable> = Mutex::new(FdTable::new());

/// File descriptor table
#[derive(Clone)]
pub struct FdTable {
    fds: [Option<FileDescriptor>; MAX_FDS],
}

impl FdTable {
    /// Create a new empty file descriptor table
    pub const fn new() -> Self {
        Self {
            fds: [const { None }; MAX_FDS],
        }
    }

    /// Initialize the fd table with stdin/stdout/stderr
    pub fn with_standard() -> Self {
        let mut table = Self::new();
        table.fds[STDIN_FD] = Some(FileDescriptor::Uart(UartFd::new(UartMode::Read)));
        table.fds[STDOUT_FD] = Some(FileDescriptor::Uart(UartFd::new(UartMode::Write)));
        table.fds[STDERR_FD] = Some(FileDescriptor::Uart(UartFd::new(UartMode::Write)));
        table
    }

    /// Initialize this table with standard descriptors (used by kernel shell helpers)
    pub fn init(&mut self) {
        *self = Self::with_standard();
    }

    /// Allocate a new file descriptor
    pub fn alloc(&mut self, fd: FileDescriptor) -> Result<usize, FdError> {
        for (i, slot) in self.fds.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(fd);
                return Ok(i);
            }
        }
        Err(FdError::TooManyOpen)
    }

    /// Get a file descriptor by number
    pub fn get(&self, fd_num: usize) -> Result<&FileDescriptor, FdError> {
        if fd_num >= MAX_FDS {
            return Err(FdError::BadFd);
        }
        self.fds[fd_num].as_ref().ok_or(FdError::BadFd)
    }

    /// Get a mutable file descriptor by number
    pub fn get_mut(&mut self, fd_num: usize) -> Result<&mut FileDescriptor, FdError> {
        if fd_num >= MAX_FDS {
            return Err(FdError::BadFd);
        }
        self.fds[fd_num].as_mut().ok_or(FdError::BadFd)
    }

    /// Close a file descriptor
    pub fn close(&mut self, fd_num: usize) -> Result<(), FdError> {
        if fd_num >= MAX_FDS {
            return Err(FdError::BadFd);
        }
        let fd = self.fds[fd_num].take();
        if fd.is_none() {
            return Err(FdError::BadFd);
        }
        if let Some(FileDescriptor::Pipe(pipe_fd)) = fd {
            PIPE_TABLE
                .lock()
                .close_pipe_end(pipe_fd.pipe_id, pipe_fd.is_read_end)?;
        }
        Ok(())
    }

    /// Duplicate a file descriptor to a specific fd number
    pub fn dup2(&mut self, old_fd: usize, new_fd: usize) -> Result<(), FdError> {
        if old_fd >= MAX_FDS || new_fd >= MAX_FDS {
            return Err(FdError::BadFd);
        }
        let fd = self.fds[old_fd].as_ref().ok_or(FdError::BadFd)?;
        let cloned = fd.clone();

        // Close new_fd if it's open
        if let Some(existing) = self.fds[new_fd].take() {
            if let FileDescriptor::Pipe(pipe_fd) = existing {
                PIPE_TABLE
                    .lock()
                    .close_pipe_end(pipe_fd.pipe_id, pipe_fd.is_read_end)?;
            }
        }
        self.fds[new_fd] = Some(cloned);
        Ok(())
    }

    /// Close all open file descriptors, ignoring individual errors
    pub fn close_all(&mut self) {
        for fd_num in 0..MAX_FDS {
            let _ = self.close(fd_num);
        }
    }
}

/// File descriptor types
pub enum FileDescriptor {
    /// UART (stdin/stdout/stderr)
    Uart(UartFd),
    /// Regular file
    File(FileFd),
    /// Pipe end
    Pipe(PipeFd),
}

impl FileDescriptor {
    /// Read from this file descriptor
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FdError> {
        match self {
            FileDescriptor::Uart(uart) => uart.read(buf),
            FileDescriptor::File(file) => file.read(buf),
            FileDescriptor::Pipe(pipe) => pipe.read(buf),
        }
    }

    /// Write to this file descriptor
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FdError> {
        match self {
            FileDescriptor::Uart(uart) => uart.write(buf),
            FileDescriptor::File(file) => file.write(buf),
            FileDescriptor::Pipe(pipe) => pipe.write(buf),
        }
    }
}

impl Clone for FileDescriptor {
    fn clone(&self) -> Self {
        match self {
            FileDescriptor::Uart(u) => FileDescriptor::Uart(u.clone()),
            FileDescriptor::File(f) => FileDescriptor::File(f.clone()),
            FileDescriptor::Pipe(p) => {
                let _ = PIPE_TABLE.lock().incref(p.pipe_id, p.is_read_end);
                FileDescriptor::Pipe(p.clone())
            }
        }
    }
}

/// UART file descriptor (for stdin/stdout/stderr)
#[derive(Clone)]
pub struct UartFd {
    mode: UartMode,
}

#[derive(Clone, Copy)]
pub enum UartMode {
    Read,
    Write,
}

impl UartFd {
    pub fn new(mode: UartMode) -> Self {
        Self { mode }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FdError> {
        match self.mode {
            UartMode::Read => {
                if buf.is_empty() {
                    return Ok(0);
                }
                let byte = crate::uart::read_byte_blocking();
                buf[0] = byte;
                Ok(1)
            }
            UartMode::Write => Err(FdError::BadFd),
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FdError> {
        match self.mode {
            UartMode::Write => {
                crate::uart::write_bytes(buf);
                Ok(buf.len())
            }
            UartMode::Read => Err(FdError::BadFd),
        }
    }
}

/// Regular file descriptor
#[derive(Clone)]
pub struct FileFd {
    path: String,
    pos: usize,
    mode: FileMode,
}

#[derive(Clone, Copy)]
pub struct FileMode {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
}

impl FileMode {
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            append: false,
            create: false,
        }
    }

    pub const fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            append: false,
            create: true,
        }
    }

    pub const fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            append: false,
            create: true,
        }
    }

    pub const fn append() -> Self {
        Self {
            read: false,
            write: true,
            append: true,
            create: true,
        }
    }
}

impl FileFd {
    pub fn open(path: String, mode: FileMode) -> Result<Self, FdError> {
        // Check if file exists
        let exists = fs::read_file(&path).is_ok();

        if !exists && !mode.create {
            return Err(FdError::NotFound);
        }

        if !exists && mode.create {
            fs::create_file(&path).map_err(|e| FdError::Fs(e))?;
        }

        let pos = if mode.append {
            // Get file size for append mode
            fs::read_file(&path).map(|data| data.len()).unwrap_or(0)
        } else {
            0
        };

        Ok(Self { path, pos, mode })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FdError> {
        if !self.mode.read {
            return Err(FdError::BadFd);
        }

        let contents = fs::read_file(&self.path).map_err(FdError::Fs)?;

        if self.pos >= contents.len() {
            return Ok(0); // EOF
        }

        let available = contents.len() - self.pos;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&contents[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FdError> {
        if !self.mode.write {
            return Err(FdError::BadFd);
        }

        if self.mode.append {
            // Append mode: read existing content, append, write back
            let mut contents = fs::read_file(&self.path).unwrap_or_else(|_| Vec::new());
            contents.extend_from_slice(buf);
            fs::write_file(&self.path, &contents).map_err(FdError::Fs)?;
            self.pos = contents.len();
        } else {
            // Write mode: for now, just overwrite the whole file
            // TODO: Support proper seeking and partial writes
            fs::write_file(&self.path, buf).map_err(FdError::Fs)?;
            self.pos = buf.len();
        }

        Ok(buf.len())
    }
}

/// Pipe file descriptor
#[derive(Clone)]
pub struct PipeFd {
    pub pipe_id: usize,
    is_read_end: bool,
}

impl PipeFd {
    pub fn new(pipe_id: usize, is_read_end: bool) -> Self {
        Self {
            pipe_id,
            is_read_end,
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FdError> {
        if !self.is_read_end {
            return Err(FdError::BadFd);
        }
        PIPE_TABLE.lock().read(self.pipe_id, buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FdError> {
        if self.is_read_end {
            return Err(FdError::BadFd);
        }
        PIPE_TABLE.lock().write(self.pipe_id, buf)
    }
}

/// Maximum number of pipes
const MAX_PIPES: usize = 8;

/// Pipe buffer size (4KB)
const PIPE_BUF_SIZE: usize = 4096;

/// Global pipe table
pub static PIPE_TABLE: Mutex<PipeTable> = Mutex::new(PipeTable::new());

/// Pipe table
pub struct PipeTable {
    pipes: [Option<Pipe>; MAX_PIPES],
}

impl PipeTable {
    pub const fn new() -> Self {
        Self {
            pipes: [const { None }; MAX_PIPES],
        }
    }

    /// Create a new pipe and return its ID
    pub fn create_pipe() -> Result<usize, FdError> {
        let mut table = PIPE_TABLE.lock();
        for (i, slot) in table.pipes.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(Pipe::new());
                return Ok(i);
            }
        }
        Err(FdError::TooManyOpen)
    }

    /// Increment refcount when cloning/duplicating a pipe end
    pub fn incref(&mut self, pipe_id: usize, is_read_end: bool) -> Result<(), FdError> {
        if pipe_id >= MAX_PIPES {
            return Err(FdError::BadFd);
        }
        let pipe = self.pipes[pipe_id].as_mut().ok_or(FdError::BadFd)?;
        if is_read_end {
            pipe.read_refcount = pipe.read_refcount.saturating_add(1);
            pipe.read_end_open = true;
        } else {
            pipe.write_refcount = pipe.write_refcount.saturating_add(1);
            pipe.write_end_open = true;
        }
        Ok(())
    }

    /// Read from a pipe
    pub fn read(&mut self, pipe_id: usize, buf: &mut [u8]) -> Result<usize, FdError> {
        if pipe_id >= MAX_PIPES {
            return Err(FdError::BadFd);
        }
        let pipe = self.pipes[pipe_id].as_mut().ok_or(FdError::BadFd)?;
        let bytes = pipe.read(buf)?;
        if bytes > 0 {
            pipe.wake_writers();
        }
        Ok(bytes)
    }

    /// Write to a pipe
    pub fn write(&mut self, pipe_id: usize, buf: &[u8]) -> Result<usize, FdError> {
        if pipe_id >= MAX_PIPES {
            return Err(FdError::BadFd);
        }
        let pipe = self.pipes[pipe_id].as_mut().ok_or(FdError::BadFd)?;
        let written = pipe.write(buf)?;
        if written > 0 {
            pipe.wake_readers();
        }
        Ok(written)
    }

    /// Register a reader that will block on this pipe
    pub fn mark_reader_waiting(&mut self, pipe_id: usize, pid: Pid) -> Result<(), FdError> {
        if pipe_id >= MAX_PIPES {
            return Err(FdError::BadFd);
        }
        let pipe = self.pipes[pipe_id].as_mut().ok_or(FdError::BadFd)?;
        pipe.mark_reader_waiting(pid);
        Ok(())
    }

    /// Register a writer that will block on this pipe
    pub fn mark_writer_waiting(&mut self, pipe_id: usize, pid: Pid) -> Result<(), FdError> {
        if pipe_id >= MAX_PIPES {
            return Err(FdError::BadFd);
        }
        let pipe = self.pipes[pipe_id].as_mut().ok_or(FdError::BadFd)?;
        pipe.mark_writer_waiting(pid);
        Ok(())
    }

    /// Close a pipe end
    pub fn close_pipe_end(&mut self, pipe_id: usize, is_read_end: bool) -> Result<(), FdError> {
        if pipe_id >= MAX_PIPES {
            return Err(FdError::BadFd);
        }
        if let Some(pipe) = &mut self.pipes[pipe_id] {
            if is_read_end {
                if pipe.read_refcount > 0 {
                    pipe.read_refcount -= 1;
                }
                pipe.read_end_open = pipe.read_refcount > 0;
                if !pipe.read_end_open {
                    pipe.wake_writers();
                }
            } else {
                if pipe.write_refcount > 0 {
                    pipe.write_refcount -= 1;
                }
                pipe.write_end_open = pipe.write_refcount > 0;
                if !pipe.write_end_open {
                    pipe.wake_readers();
                }
            }

            // Clean up pipe if both ends are closed
            if !pipe.read_end_open && !pipe.write_end_open {
                self.pipes[pipe_id] = None;
            }
        }
        Ok(())
    }
}

/// Pipe structure with ring buffer
pub struct Pipe {
    buffer: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
    read_end_open: bool,
    write_end_open: bool,
    read_refcount: usize,
    write_refcount: usize,
    waiting_readers: Vec<Pid>,
    waiting_writers: Vec<Pid>,
}

impl Pipe {
    pub fn new() -> Self {
        Self {
            buffer: alloc::vec![0u8; PIPE_BUF_SIZE],
            read_pos: 0,
            write_pos: 0,
            read_end_open: true,
            write_end_open: true,
            read_refcount: 1,
            write_refcount: 1,
            waiting_readers: Vec::new(),
            waiting_writers: Vec::new(),
        }
    }

    /// Get number of bytes available to read
    fn available(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            PIPE_BUF_SIZE - self.read_pos + self.write_pos
        }
    }

    /// Get number of bytes available to write
    fn space_available(&self) -> usize {
        PIPE_BUF_SIZE - self.available() - 1 // -1 to distinguish full from empty
    }

    /// Read from pipe
    /// Returns WouldBlock if pipe is empty and write end is still open
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FdError> {
        let available = self.available();

        if available == 0 {
            if self.write_end_open {
                // Pipe is empty but write end is open - would block
                return Err(FdError::WouldBlock);
            } else {
                // Write end closed, return EOF
                return Ok(0);
            }
        }

        let to_read = buf.len().min(available);
        let mut bytes_read = 0;

        while bytes_read < to_read {
            buf[bytes_read] = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % PIPE_BUF_SIZE;
            bytes_read += 1;
        }

        Ok(bytes_read)
    }

    /// Write to pipe
    /// Returns WouldBlock if pipe is full
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FdError> {
        if !self.read_end_open {
            // Read end closed - would cause SIGPIPE in Unix
            return Err(FdError::BrokenPipe);
        }

        let space = self.space_available();

        if space == 0 {
            // Pipe is full - would block
            return Err(FdError::WouldBlock);
        }

        let to_write = buf.len().min(space);
        let mut bytes_written = 0;

        while bytes_written < to_write {
            self.buffer[self.write_pos] = buf[bytes_written];
            self.write_pos = (self.write_pos + 1) % PIPE_BUF_SIZE;
            bytes_written += 1;
        }

        Ok(bytes_written)
    }

    fn mark_reader_waiting(&mut self, pid: Pid) {
        if !self.waiting_readers.contains(&pid) {
            self.waiting_readers.push(pid);
        }
    }

    fn mark_writer_waiting(&mut self, pid: Pid) {
        if !self.waiting_writers.contains(&pid) {
            self.waiting_writers.push(pid);
        }
    }

    fn wake_readers(&mut self) {
        let readers = core::mem::take(&mut self.waiting_readers);
        for pid in readers {
            Scheduler::unblock(pid);
        }
    }

    fn wake_writers(&mut self) {
        let writers = core::mem::take(&mut self.waiting_writers);
        for pid in writers {
            Scheduler::unblock(pid);
        }
    }
}

/// File descriptor errors
#[derive(Debug, Clone, Copy)]
pub enum FdError {
    BadFd,
    TooManyOpen,
    NotFound,
    NotImplemented,
    WouldBlock,
    BrokenPipe,
    Fs(fs::FsError),
}

impl fmt::Display for FdError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FdError::BadFd => write!(f, "Bad file descriptor"),
            FdError::TooManyOpen => write!(f, "Too many open files"),
            FdError::NotFound => write!(f, "File not found"),
            FdError::NotImplemented => write!(f, "Not implemented"),
            FdError::WouldBlock => write!(f, "Operation would block"),
            FdError::BrokenPipe => write!(f, "Broken pipe"),
            FdError::Fs(err) => write!(f, "Filesystem error: {:?}", err),
        }
    }
}
