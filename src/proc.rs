use alloc::{string::String, vec::Vec};
use spin::Mutex;

/// Maximum number of concurrent processes
pub const MAX_PROCESSES: usize = 8;

/// Process ID type
pub type Pid = usize;

/// Invalid/null PID
pub const INVALID_PID: Pid = usize::MAX;

/// Process states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is currently running
    Running,
    /// Process is ready to run
    Ready,
    /// Process is blocked waiting for I/O
    Blocked,
    /// Process has exited
    Exited,
}

/// Process control block
#[derive(Clone)]
pub struct Process {
    /// Process ID
    pub pid: Pid,
    /// Parent process ID
    pub parent_pid: Pid,
    /// Process state
    pub state: ProcessState,
    /// Exit code (only valid if state == Exited)
    pub exit_code: isize,
    /// Program entry point
    pub entry: u64,
    /// Stack top
    pub stack_top: u64,
    /// Saved program counter
    pub pc: usize,
    /// Saved stack pointer
    pub sp: usize,
    /// Saved registers (x1-x31, excluding x0 which is always 0)
    pub regs: [usize; 31],
    /// Program path (for debugging)
    pub path: String,
    /// Command-line arguments
    pub args: Vec<String>,
    /// File descriptors for this process
    pub fd_table: crate::fd::FdTable,
    /// Memory snapshot of the user window (stored when process is not running)
    pub memory: Vec<u8>,
    /// Initial argc value (for newly spawned processes)
    pub argc: usize,
    /// Initial argv pointer (for newly spawned processes)
    pub started: bool,
    /// Initial argv pointer (for newly spawned processes)
    pub argv_ptr: usize,
}

impl Process {
    /// Create a new process
    pub fn new(
        pid: Pid,
        parent_pid: Pid,
        entry: u64,
        stack_top: u64,
        path: String,
        args: Vec<String>,
        fd_table: crate::fd::FdTable,
        memory: Vec<u8>,
        argc: usize,
        argv_ptr: usize,
    ) -> Self {
        Self {
            pid,
            parent_pid,
            state: ProcessState::Ready,
            exit_code: 0,
            entry,
            stack_top,
            pc: entry as usize,
            sp: stack_top as usize,
            regs: [0; 31],
            path,
            args,
            fd_table,
            memory,
            argc,
            argv_ptr,
            started: false,
        }
    }

    /// Mark process as exited with given code
    pub fn exit(&mut self, code: isize) {
        self.state = ProcessState::Exited;
        self.exit_code = code;
    }

    /// Check if process is running
    pub fn is_running(&self) -> bool {
        self.state == ProcessState::Running
    }

    /// Check if process has exited
    pub fn has_exited(&self) -> bool {
        self.state == ProcessState::Exited
    }
}

/// Global process table
pub static PROCESS_TABLE: Mutex<ProcessTable> = Mutex::new(ProcessTable::new());

/// Process table managing all processes
pub struct ProcessTable {
    /// Array of processes
    processes: [Option<Process>; MAX_PROCESSES],
    /// Currently running process ID
    current_pid: Pid,
    /// Next PID to allocate
    next_pid: Pid,
}

impl ProcessTable {
    /// Create a new empty process table
    pub const fn new() -> Self {
        Self {
            processes: [const { None }; MAX_PROCESSES],
            current_pid: INVALID_PID,
            next_pid: 1, // PID 0 is reserved for kernel
        }
    }

    /// Allocate a new PID
    fn alloc_pid(&mut self) -> Pid {
        let pid = self.next_pid;
        self.next_pid += 1;
        pid
    }

    /// Find a free process slot
    fn find_free_slot(&mut self) -> Option<usize> {
        self.processes.iter().position(|p| p.is_none())
    }

    /// Create a new process
    pub fn spawn(
        &mut self,
        entry: u64,
        stack_top: u64,
        path: String,
        args: Vec<String>,
        fd_table: crate::fd::FdTable,
        memory: Vec<u8>,
        argc: usize,
        argv_ptr: usize,
    ) -> Result<Pid, SpawnError> {
        let slot = self.find_free_slot().ok_or(SpawnError::TooManyProcesses)?;
        let pid = self.alloc_pid();
        let parent_pid = self.current_pid;

        let process = Process::new(
            pid,
            parent_pid,
            entry,
            stack_top,
            path,
            args,
            fd_table,
            memory,
            argc,
            argv_ptr,
        );
        self.processes[slot] = Some(process);

        Ok(pid)
    }

    /// Get a process by PID
    pub fn get(&self, pid: Pid) -> Option<&Process> {
        self.processes.iter().find_map(|p| {
            p.as_ref().and_then(|proc| {
                if proc.pid == pid {
                    Some(proc)
                } else {
                    None
                }
            })
        })
    }

    /// Get a mutable process by PID
    pub fn get_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        self.processes.iter_mut().find_map(|p| {
            p.as_mut().and_then(|proc| {
                if proc.pid == pid {
                    Some(proc)
                } else {
                    None
                }
            })
        })
    }

    /// Get the currently running process
    pub fn current(&self) -> Option<&Process> {
        if self.current_pid == INVALID_PID {
            None
        } else {
            self.get(self.current_pid)
        }
    }

    /// Get the currently running process mutably
    pub fn current_mut(&mut self) -> Option<&mut Process> {
        if self.current_pid == INVALID_PID {
            None
        } else {
            self.get_mut(self.current_pid)
        }
    }

    /// Set the current running process
    pub fn set_current(&mut self, pid: Pid) {
        self.current_pid = pid;
    }

    /// Get the current process PID
    pub fn get_current_pid(&self) -> Pid {
        self.current_pid
    }

    /// Mark a process as exited
    pub fn exit_process(&mut self, pid: Pid, code: isize) {
        if let Some(process) = self.get_mut(pid) {
            process.fd_table.close_all();
            process.exit(code);
        }
    }

    /// Wait for a child process to exit
    /// Returns (child_pid, exit_code) if a child has exited, None if no children or still running
    pub fn wait(&mut self, parent_pid: Pid) -> Option<(Pid, isize)> {
        // Find any exited child process
        for process in self.processes.iter_mut().flatten() {
            if process.parent_pid == parent_pid && process.has_exited() {
                let child_pid = process.pid;
                let exit_code = process.exit_code;
                // Remove the exited process from the table
                if let Some(slot) = self
                    .processes
                    .iter()
                    .position(|p| p.as_ref().map(|pr| pr.pid) == Some(child_pid))
                {
                    self.processes[slot] = None;
                }
                return Some((child_pid, exit_code));
            }
        }

        None
    }

    /// Check if a process has any children
    pub fn has_children(&self, parent_pid: Pid) -> bool {
        self.processes
            .iter()
            .flatten()
            .any(|p| p.parent_pid == parent_pid)
    }

    /// Get all children of a process
    pub fn get_children(&self, parent_pid: Pid) -> Vec<Pid> {
        self.processes
            .iter()
            .flatten()
            .filter(|p| p.parent_pid == parent_pid)
            .map(|p| p.pid)
            .collect()
    }

    /// Clean up all processes
    pub fn clear(&mut self) {
        self.processes = [const { None }; MAX_PROCESSES];
        self.current_pid = INVALID_PID;
    }

    /// Get all processes (for scheduling)
    pub fn get_all_processes(&self) -> Vec<&Process> {
        self.processes.iter().filter_map(|p| p.as_ref()).collect()
    }

    /// Save the current process's memory from the user window
    pub fn save_current_memory(&mut self) {
        if self.current_pid == INVALID_PID {
            return;
        }
        if let Some(process) = self.get_mut(self.current_pid) {
            process.memory.clear();
            process.memory.resize(crate::process::USER_WINDOW_SIZE, 0);
            crate::process::snapshot_user_window(&mut process.memory);
        }
    }

    /// Restore a process's memory into the user window
    pub fn restore_process_memory(&self, pid: Pid) {
        if let Some(process) = self.get(pid) {
            if !process.memory.is_empty() {
                crate::process::restore_user_window(&process.memory);
            }
        }
    }

    /// Save the current process's state
    /// Saves PC and SP which are needed to resume execution
    pub fn save_current_registers(&mut self, _trap_frame: &riscv_rt::TrapFrame) {
        if self.current_pid == INVALID_PID {
            return;
        }
        if let Some(process) = self.get_mut(self.current_pid) {
            // Save PC - this is where the process will resume
            process.pc = unsafe { riscv::register::sepc::read() };

            // Save SP - it's stored in sscratch CSR
            process.sp = unsafe {
                let sp: usize;
                core::arch::asm!(
                    "csrr {0}, sscratch",
                    out(reg) sp
                );
                sp
            };
        }
    }

    /// Restore a process's state to resume execution
    /// This sets up the trap frame to return to the process
    pub fn restore_process_registers(&mut self, pid: Pid, trap_frame: &mut riscv_rt::TrapFrame) {
        if let Some(process) = self.get_mut(pid) {
            // Restore PC - this is where we'll return to
            unsafe { riscv::register::sepc::write(process.pc) };

            // Restore SP - it's stored in sscratch CSR
            // The trap handler swaps SP with sscratch on entry/exit
            unsafe {
                core::arch::asm!(
                    "csrw sscratch, {0}",
                    in(reg) process.sp
                );
            }

            // For newly spawned processes, initialize trap frame registers
            // Set argc and argv for the process's _start function
            if !process.started {
                trap_frame.ra = 0;
                trap_frame.a0 = process.argc;
                trap_frame.a1 = process.argv_ptr;
                trap_frame.a2 = 0;
                trap_frame.a3 = 0;
                trap_frame.a4 = 0;
                trap_frame.a5 = 0;
                trap_frame.a6 = 0;
                trap_frame.a7 = 0;
                trap_frame.t0 = 0;
                trap_frame.t1 = 0;
                trap_frame.t2 = 0;
                trap_frame.t3 = 0;
                trap_frame.t4 = 0;
                trap_frame.t5 = 0;
                trap_frame.t6 = 0;
                process.started = true;
            }
        }
    }
}

/// Errors that can occur during process spawning
#[derive(Debug, Clone, Copy)]
pub enum SpawnError {
    /// Too many processes already running
    TooManyProcesses,
    /// Invalid program path or file not found
    ProgramNotFound,
    /// Failed to load program
    LoadFailed,
    /// Out of memory
    OutOfMemory,
}
