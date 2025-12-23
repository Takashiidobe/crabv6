use alloc::{format, vec, vec::Vec};
use core::ptr;

use riscv::register::sstatus::{self, SPP};
use riscv_rt::TrapFrame;

use crate::{elf::ElfFile, fs, uart};

const USER_IMAGE_BASE: u64 = 0x8040_0000;
const USER_IMAGE_LIMIT: u64 = USER_IMAGE_BASE + 0x0002_0000; // 128 KiB window
const USER_STACK_SIZE: usize = 8 * 1024;

#[unsafe(no_mangle)]
static mut KERNEL_STACK_POINTER: usize = 0;
#[unsafe(no_mangle)]
static mut KERNEL_RETURN_ADDRESS: usize = 0;

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

pub unsafe fn enter_user(program: &LoadedProgram, args: &[&str]) -> isize { unsafe {
    // Copy program segments
    for seg in &program.segments {
        unsafe {
            ptr::copy_nonoverlapping(seg.data.as_ptr(), seg.dest, seg.data.len());
        }
    }

    // Clear user stack
    let stack_base = (program.stack_top - USER_STACK_SIZE as u64) as *mut u8;
    unsafe {
        ptr::write_bytes(stack_base, 0, USER_STACK_SIZE);
    }

    // Stack grows downward
    let mut sp = program.stack_top as usize;
    let argc = args.len();
    debug_assert!(
        argc <= 16,
        "too many arguments passed to user program (max 16 supported)"
    );
    let mut arg_ptrs: [usize; 16] = [0; 16]; // support up to 16 args

    // Copy arguments into stack (in reverse order)
    for (index, &arg) in args.iter().enumerate().rev() {
        let bytes = arg.as_bytes();
        sp -= bytes.len() + 1;
        copy_to_user(sp as *mut u8, bytes.as_ptr(), bytes.len());
        write_byte_to_user((sp + bytes.len()) as *mut u8, 0);
        arg_ptrs[index] = sp;
    }

    // Align stack pointer to 16 bytes before pushing pointer-sized values.
    sp &= !(core::mem::size_of::<usize>() * 2 - 1);

    // Ensure the total number of pointer-sized pushes keeps the stack aligned.
    let pointer_pushes = argc + 2; // argv entries + NULL + argc
    if pointer_pushes & 1 != 0 {
        sp -= core::mem::size_of::<usize>();
        write_usize_to_user(sp as *mut usize, 0); // padding
    }

    // Push NULL terminator
    sp -= core::mem::size_of::<usize>();
    write_usize_to_user(sp as *mut usize, 0);

    // Push argv pointers (reverse back to original order)
    for &ptr in arg_ptrs[..argc].iter().rev() {
        sp -= core::mem::size_of::<usize>();
        write_usize_to_user(sp as *mut usize, ptr);
    }
    let argv_ptr = sp;

    // Push argc
    sp -= core::mem::size_of::<usize>();
    write_usize_to_user(sp as *mut usize, argc);

    // Prepare to enter user mode
    unsafe {
        sstatus::set_spp(SPP::User);
        sstatus::set_spie();
    }

    let entry = program.entry as usize;

    unsafe { enter_user_trampoline(entry, sp, argc, argv_ptr) }
}}

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
