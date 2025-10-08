use alloc::vec::Vec;
use core::mem::size_of;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_LSB: u8 = 1;
const ELF_VERSION: u8 = 1;

const PT_LOAD: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    BadMagic,
    UnsupportedClass,
    UnsupportedEncoding,
    UnsupportedVersion,
    Truncated,
    UnsupportedType,
}

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub vaddr: u64,
    pub mem_size: u64,
    pub file_size: u64,
    pub file_offset: u64,
    pub align: u64,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct ElfFile {
    pub entry: u64,
    pub segments: Vec<Segment>,
    pub data: Vec<u8>,
}

#[repr(C)]
struct Elf64Header {
    ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

impl ElfFile {
    pub fn parse(data: &[u8]) -> Result<Self, ElfError> {
        if data.len() < size_of::<Elf64Header>() {
            return Err(ElfError::Truncated);
        }
        let mut hdr_buf = [0u8; size_of::<Elf64Header>()];
        hdr_buf.copy_from_slice(&data[..size_of::<Elf64Header>()]);
        let header = parse_header(&hdr_buf)?;

        if header.e_phentsize as usize != size_of::<Elf64ProgramHeader>() {
            return Err(ElfError::UnsupportedVersion);
        }

        let mut segments = Vec::new();
        let phoff = header.e_phoff as usize;
        let phentsize = header.e_phentsize as usize;
        let phcount = header.e_phnum as usize;

        if phoff + phcount * phentsize > data.len() {
            return Err(ElfError::Truncated);
        }

        for idx in 0..phcount {
            let start = phoff + idx * phentsize;
            let end = start + phentsize;
            let mut buf = [0u8; size_of::<Elf64ProgramHeader>()];
            buf.copy_from_slice(&data[start..end]);
            let ph = parse_program_header(&buf);
            if ph.p_type == PT_LOAD {
                segments.push(Segment {
                    vaddr: ph.p_vaddr,
                    mem_size: ph.p_memsz,
                    file_size: ph.p_filesz,
                    file_offset: ph.p_offset,
                    align: ph.p_align,
                    flags: ph.p_flags,
                });
            }
        }

        Ok(Self {
            entry: header.e_entry,
            segments,
            data: data.to_vec(),
        })
    }
}

fn parse_header(buf: &[u8]) -> Result<Elf64Header, ElfError> {
    let mut hdr = Elf64Header {
        ident: [0; 16],
        e_type: 0,
        e_machine: 0,
        e_version: 0,
        e_entry: 0,
        e_phoff: 0,
        e_shoff: 0,
        e_flags: 0,
        e_ehsize: 0,
        e_phentsize: 0,
        e_phnum: 0,
        e_shentsize: 0,
        e_shnum: 0,
        e_shstrndx: 0,
    };

    hdr.ident.copy_from_slice(&buf[0..16]);
    if hdr.ident[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if hdr.ident[4] != ELF_CLASS_64 {
        return Err(ElfError::UnsupportedClass);
    }
    if hdr.ident[5] != ELF_DATA_LSB {
        return Err(ElfError::UnsupportedEncoding);
    }
    if hdr.ident[6] != ELF_VERSION {
        return Err(ElfError::UnsupportedVersion);
    }

    hdr.e_type = u16::from_le_bytes(buf[16..18].try_into().unwrap());
    hdr.e_machine = u16::from_le_bytes(buf[18..20].try_into().unwrap());
    hdr.e_version = u32::from_le_bytes(buf[20..24].try_into().unwrap());
    hdr.e_entry = u64::from_le_bytes(buf[24..32].try_into().unwrap());
    hdr.e_phoff = u64::from_le_bytes(buf[32..40].try_into().unwrap());
    hdr.e_shoff = u64::from_le_bytes(buf[40..48].try_into().unwrap());
    hdr.e_flags = u32::from_le_bytes(buf[48..52].try_into().unwrap());
    hdr.e_ehsize = u16::from_le_bytes(buf[52..54].try_into().unwrap());
    hdr.e_phentsize = u16::from_le_bytes(buf[54..56].try_into().unwrap());
    hdr.e_phnum = u16::from_le_bytes(buf[56..58].try_into().unwrap());
    hdr.e_shentsize = u16::from_le_bytes(buf[58..60].try_into().unwrap());
    hdr.e_shnum = u16::from_le_bytes(buf[60..62].try_into().unwrap());
    hdr.e_shstrndx = u16::from_le_bytes(buf[62..64].try_into().unwrap());

    Ok(hdr)
}

fn parse_program_header(buf: &[u8]) -> Elf64ProgramHeader {
    Elf64ProgramHeader {
        p_type: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
        p_flags: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
        p_offset: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
        p_vaddr: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
        p_paddr: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
        p_filesz: u64::from_le_bytes(buf[32..40].try_into().unwrap()),
        p_memsz: u64::from_le_bytes(buf[40..48].try_into().unwrap()),
        p_align: u64::from_le_bytes(buf[48..56].try_into().unwrap()),
    }
}
