use crate::proc::{INVALID_PID, PROCESS_TABLE, Pid, ProcessState};
use alloc::vec::Vec;

/// Simple round-robin scheduler
pub struct Scheduler;

impl Scheduler {
    /// Select the next process to run
    /// Returns None if no processes are ready
    pub fn schedule() -> Option<Pid> {
        let mut table = PROCESS_TABLE.lock();
        let current_pid = table.get_current_pid();

        // Find next ready process after current
        let processes: Vec<_> = table
            .get_all_processes()
            .iter()
            .filter(|p| p.state == ProcessState::Ready || p.state == ProcessState::Running)
            .map(|p| p.pid)
            .collect();

        if processes.is_empty() {
            return None;
        }

        // Round-robin: find next process after current
        if current_pid != INVALID_PID {
            if let Some(current_idx) = processes.iter().position(|&pid| pid == current_pid) {
                let next_idx = (current_idx + 1) % processes.len();
                return Some(processes[next_idx]);
            }
        }

        // No current process or not found, return first ready process
        Some(processes[0])
    }

    /// Yield CPU to another process
    pub fn yield_cpu() {
        // Mark current as Ready
        let current_pid = PROCESS_TABLE.lock().get_current_pid();
        if current_pid != INVALID_PID {
            if let Some(process) = PROCESS_TABLE.lock().get_mut(current_pid) {
                if process.state == ProcessState::Running {
                    process.state = ProcessState::Ready;
                }
            }
        }

        // Schedule next process
        if let Some(next_pid) = Self::schedule() {
            PROCESS_TABLE.lock().set_current(next_pid);
            if let Some(process) = PROCESS_TABLE.lock().get_mut(next_pid) {
                process.state = ProcessState::Running;
            }
        }
    }

    /// Block the current process
    pub fn block_current() {
        let current_pid = PROCESS_TABLE.lock().get_current_pid();
        if current_pid != INVALID_PID {
            if let Some(process) = PROCESS_TABLE.lock().get_mut(current_pid) {
                process.state = ProcessState::Blocked;
            }
        }
    }

    /// Unblock a specific process
    pub fn unblock(pid: Pid) {
        if let Some(process) = PROCESS_TABLE.lock().get_mut(pid) {
            if process.state == ProcessState::Blocked {
                process.state = ProcessState::Ready;
            }
        }
    }

    /// Perform a full context switch if needed
    /// This should be called after syscalls that might block or when yielding
    /// Returns true if a context switch occurred
    pub fn maybe_switch(trap_frame: &mut riscv_rt::TrapFrame) -> bool {
        let current_pid = PROCESS_TABLE.lock().get_current_pid();

        // Determine if we should switch
        let (should_switch, make_current_ready) = if current_pid == INVALID_PID {
            (true, false)
        } else {
            let mut table = PROCESS_TABLE.lock();
            let state = table.get(current_pid).map(|p| p.state);
            let has_other_ready = table.get_all_processes().iter().any(|p| {
                p.pid != current_pid
                    && (p.state == ProcessState::Ready || p.state == ProcessState::Running)
            });
            match state {
                Some(ProcessState::Blocked) | Some(ProcessState::Exited) | None => (true, false),
                Some(ProcessState::Running) | Some(ProcessState::Ready) => {
                    if has_other_ready {
                        (true, true)
                    } else {
                        (false, false)
                    }
                }
            }
        };

        if !should_switch {
            return false;
        }

        // Save current process state if there is one
        if current_pid != INVALID_PID {
            let mut table = PROCESS_TABLE.lock();
            table.save_current_registers(trap_frame);
            table.save_current_memory();
            if make_current_ready {
                if let Some(proc) = table.get_mut(current_pid) {
                    if proc.state == ProcessState::Running {
                        proc.state = ProcessState::Ready;
                    }
                }
            }
        }

        // Schedule next process
        if let Some(next_pid) = Self::schedule() {
            // Restore next process state
            let mut table = PROCESS_TABLE.lock();
            table.set_current(next_pid);
            table.restore_process_memory(next_pid);
            table.restore_process_registers(next_pid, trap_frame);

            // Mark as running
            if let Some(process) = table.get_mut(next_pid) {
                process.state = ProcessState::Running;
            }

            true
        } else {
            // No runnable processes - stay in kernel or idle
            false
        }
    }
}
