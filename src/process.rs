use alloc::{format, vec, vec::Vec};
use core::ptr;

use riscv::register::sstatus::{self, SPP};
use riscv_rt::TrapFrame;

use crate::{elf::ElfFile, fs, uart};

const USER_IMAGE_BASE: u64 = 0x8040_0000;
const USER_IMAGE_LIMIT: u64 = USER_IMAGE_BASE + 0x0002_0000; // 128 KiB window
const USER_STACK_SIZE: usize = 8 * 1024;
pub const USER_WINDOW_SIZE: usize = (USER_IMAGE_LIMIT - USER_IMAGE_BASE) as usize;

#[unsafe(no_mangle)]
static mut KERNEL_STACK_POINTER: usize = 0;
#[unsafe(no_mangle)]
static mut KERNEL_RETURN_ADDRESS: usize = 0;
static mut USER_SNAPSHOT: [u8; USER_WINDOW_SIZE] = [0; USER_WINDOW_SIZE];

unsafe extern "C" {
    fn enter_user_trampoline(entry: usize, stack_top: usize, argc: usize, argv: usize) -> isize;
    fn kernel_resume_from_user();
}

#[derive(Debug)]
pub enum LoadError {
    Fs(crate::fs::FsError),
    Elf(crate::elf::ElfError),
    OutOfMemory,
}

#[derive(Debug)]
pub struct LoadedProgram {
    pub entry: u64,
    pub stack_top: u64,
    pub segments: Vec<SegmentImage>,
}

#[derive(Debug)]
pub struct SegmentImage {
    pub dest: *mut u8,
    pub data: Vec<u8>,
    pub flags: u32,
}

pub fn load(path: &str) -> Result<LoadedProgram, LoadError> {
    let bytes = fs::read_file(path).map_err(LoadError::Fs)?;
    let elf = ElfFile::parse(&bytes).map_err(LoadError::Elf)?;

    let base_vaddr = elf
        .segments
        .iter()
        .map(|seg| seg.vaddr)
        .min()
        .unwrap_or(elf.entry);

    let mut segments = Vec::new();
    for seg in &elf.segments {
        let mut data = vec![0u8; seg.mem_size as usize];
        if seg.file_size > 0 {
            let start = seg.file_offset as usize;
            let end = start + seg.file_size as usize;
            data[..seg.file_size as usize].copy_from_slice(&elf.data[start..end]);
        }

        let offset = seg.vaddr.saturating_sub(base_vaddr);
        let dest_addr = USER_IMAGE_BASE + offset;
        let dest_end = dest_addr + data.len() as u64;
        if dest_end > USER_IMAGE_LIMIT {
            return Err(LoadError::OutOfMemory);
        }

        segments.push(SegmentImage {
            dest: dest_addr as *mut u8,
            data,
            flags: seg.flags,
        });
    }

    let entry = USER_IMAGE_BASE + elf.entry.saturating_sub(base_vaddr);
    let stack_top = USER_IMAGE_LIMIT;

    Ok(LoadedProgram {
        entry,
        stack_top,
        segments,
    })
}

pub fn dump(program: &LoadedProgram) {
    uart::write_str("Loaded program:\n");
    uart::write_str(&format!(" entry: 0x{:x}\n", program.entry));
    for seg in &program.segments {
        uart::write_str(&format!(
            "  segment @0x{:x}, {} bytes (flags 0x{:x})\n",
            seg.dest as usize,
            seg.data.len(),
            seg.flags
        ));
    }
}

/// Load program segments directly into the user window.
pub fn load_into_user_window(program: &LoadedProgram) -> Result<(), LoadError> {
    for seg in &program.segments {
        let dest = seg.dest as usize;
        let offset = dest.saturating_sub(USER_IMAGE_BASE as usize);
        let end = offset + seg.data.len();
        if end > USER_WINDOW_SIZE {
            return Err(LoadError::OutOfMemory);
        }
        unsafe {
            ptr::copy_nonoverlapping(
                seg.data.as_ptr(),
                (USER_IMAGE_BASE as usize + offset) as *mut u8,
                seg.data.len(),
            );
        }
    }
    Ok(())
}

/// Build the user stack in place inside the user window.
pub fn build_user_stack(args: &[&str]) -> Result<(usize, usize, usize), LoadError> {
    let mut sp = USER_IMAGE_LIMIT as usize;
    let argc = args.len();
    debug_assert!(argc <= 16, "too many arguments (max 16)");
    let mut arg_ptrs: [usize; 16] = [0; 16];

    uart::write_str(&format!("[build_user_stack] argc={}\n", argc));

    for (index, &arg) in args.iter().enumerate().rev() {
        let bytes = arg.as_bytes();
        sp = sp.saturating_sub(bytes.len() + 1);
        if sp < USER_IMAGE_BASE as usize {
            return Err(LoadError::OutOfMemory);
        }
        unsafe {
            copy_to_user(sp as *mut u8, bytes.as_ptr(), bytes.len());
            write_byte_to_user((sp + bytes.len()) as *mut u8, 0);
        }
        arg_ptrs[index] = sp;
        uart::write_str(&format!("[build_user_stack] arg[{}]='{}' at 0x{:x}\n", index, arg, sp));
    }

    sp &= !(core::mem::size_of::<usize>() * 2 - 1);

    let pointer_pushes = argc + 2;
    if pointer_pushes & 1 != 0 {
        sp = sp.saturating_sub(core::mem::size_of::<usize>());
        unsafe { write_usize_to_user(sp as *mut usize, 0) };
    }

    sp = sp.saturating_sub(core::mem::size_of::<usize>());
    unsafe { write_usize_to_user(sp as *mut usize, 0) };

    uart::write_str(&format!("[build_user_stack] writing argv array at sp=0x{:x}\n", sp));
    for (i, &ptr) in arg_ptrs[..argc].iter().rev().enumerate() {
        sp = sp.saturating_sub(core::mem::size_of::<usize>());
        unsafe { write_usize_to_user(sp as *mut usize, ptr) };
        uart::write_str(&format!("[build_user_stack] argv[{}]=0x{:x} written at sp=0x{:x}\n", argc - 1 - i, ptr, sp));
    }
    let argv_ptr = sp;

    sp = sp.saturating_sub(core::mem::size_of::<usize>());
    unsafe { write_usize_to_user(sp as *mut usize, argc) };

    uart::write_str(&format!("[build_user_stack] returning sp=0x{:x}, argc={}, argv_ptr=0x{:x}\n", sp, argc, argv_ptr));

    Ok((sp, argc, argv_ptr))
}

/// Compatibility helper: load a program and enter user mode immediately.
pub unsafe fn enter_user(program: &LoadedProgram, args: &[&str]) -> isize {
    load_into_user_window(program).expect("load_into_user_window failed");
    let (sp, argc, argv_ptr) = build_user_stack(args).expect("build_user_stack failed");
    unsafe { enter_user_at(program.entry as usize, sp, argc, argv_ptr) }
}

/// Enter user mode using a pre-built memory image already loaded into the user window.
pub unsafe fn enter_user_at(entry: usize, sp: usize, argc: usize, argv_ptr: usize) -> isize {
    unsafe {
        sstatus::set_spp(SPP::User);
        sstatus::set_spie();
    }
    unsafe { enter_user_trampoline(entry, sp, argc, argv_ptr) }
}

/// Copy the live user window into the provided buffer.
pub fn snapshot_user_window(buf: &mut [u8]) {
    unsafe {
        ptr::copy_nonoverlapping(
            USER_IMAGE_BASE as *const u8,
            buf.as_mut_ptr(),
            USER_WINDOW_SIZE,
        );
    }
}

/// Restore the user window from a buffer.
pub fn restore_user_window(buf: &[u8]) {
    unsafe {
        ptr::copy_nonoverlapping(
            buf.as_ptr(),
            USER_IMAGE_BASE as *mut u8,
            USER_WINDOW_SIZE,
        );
    }
}

/// Snapshot the user window into a static buffer.
pub fn snapshot_user_window_static() {
    unsafe {
        let dst = core::ptr::addr_of_mut!(USER_SNAPSHOT) as *mut u8;
        ptr::copy_nonoverlapping(
            USER_IMAGE_BASE as *const u8,
            dst,
            USER_WINDOW_SIZE,
        );
    }
}

/// Restore the user window from the static buffer.
pub fn restore_user_window_static() {
    unsafe {
        let src = core::ptr::addr_of!(USER_SNAPSHOT) as *const u8;
        ptr::copy_nonoverlapping(
            src,
            USER_IMAGE_BASE as *mut u8,
            USER_WINDOW_SIZE,
        );
    }
}

pub unsafe fn prepare_for_kernel_return(trap_frame: *mut TrapFrame, code: isize) {
    unsafe {
        (*trap_frame).ra = KERNEL_RETURN_ADDRESS;
        sstatus::set_spp(SPP::Supervisor);
        riscv::register::sepc::write(kernel_resume_from_user as *const () as usize);
        // Propagate exit code back to the caller through a0 when we return to kernel mode
        (*trap_frame).a0 = code as usize;
    }
}

#[inline(always)]
unsafe fn copy_to_user(dest: *mut u8, src: *const u8, len: usize) {
    unsafe {
        ptr::copy_nonoverlapping(src, dest, len);
    }
}

#[inline(always)]
unsafe fn write_byte_to_user(dest: *mut u8, value: u8) {
    unsafe {
        ptr::write(dest, value);
    }
}

#[inline(always)]
unsafe fn write_usize_to_user(dest: *mut usize, value: usize) {
    unsafe {
        ptr::write(dest, value);
    }
}
