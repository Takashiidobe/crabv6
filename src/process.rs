use alloc::{format, vec, vec::Vec};
use core::ptr;

use riscv::register::sstatus::{self, SPP};

use crate::{elf::ElfFile, fs, uart};

const USER_IMAGE_BASE: u64 = 0x8040_0000;
const USER_IMAGE_LIMIT: u64 = USER_IMAGE_BASE + 0x0002_0000; // 128 KiB window
const USER_STACK_SIZE: usize = 8 * 1024;

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

pub unsafe fn enter_user(program: &LoadedProgram) -> ! {
    for seg in &program.segments {
        unsafe {
            ptr::copy_nonoverlapping(seg.data.as_ptr(), seg.dest, seg.data.len());
        }
    }

    let stack_base = (program.stack_top - USER_STACK_SIZE as u64) as *mut u8;
    unsafe { ptr::write_bytes(stack_base, 0, USER_STACK_SIZE) };

    unsafe {
        sstatus::set_spp(SPP::User);
        sstatus::set_spie();
    }

    let trampoline_stack = program.stack_top as usize;
    let entry = program.entry as usize;

    unsafe {
        core::arch::asm!(
            "mv sp, {stack}",
            "csrw sepc, {entry}",
            "sret",
            stack = in(reg) trampoline_stack,
            entry = in(reg) entry,
            options(noreturn)
        );
    }
}
