use alloc::vec::Vec;
use const_default::ConstDefault;
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
#[derive(ConstDefault, Debug, Clone)]
struct Elf64Header {
    ident: [u8; 16],
    r#type: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

impl TryFrom<&[u8; 64]> for Elf64Header {
    type Error = ElfError;

    fn try_from(value: &[u8; 64]) -> Result<Self, Self::Error> {
        let mut hdr = Elf64Header::DEFAULT;

        hdr.ident.copy_from_slice(&value[0..16]);
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

        hdr.r#type = u16::from_le_bytes(value[16..18].try_into().unwrap());
        hdr.machine = u16::from_le_bytes(value[18..20].try_into().unwrap());
        hdr.version = u32::from_le_bytes(value[20..24].try_into().unwrap());
        hdr.entry = u64::from_le_bytes(value[24..32].try_into().unwrap());
        hdr.phoff = u64::from_le_bytes(value[32..40].try_into().unwrap());
        hdr.shoff = u64::from_le_bytes(value[40..48].try_into().unwrap());
        hdr.flags = u32::from_le_bytes(value[48..52].try_into().unwrap());
        hdr.ehsize = u16::from_le_bytes(value[52..54].try_into().unwrap());
        hdr.phentsize = u16::from_le_bytes(value[54..56].try_into().unwrap());
        hdr.phnum = u16::from_le_bytes(value[56..58].try_into().unwrap());
        hdr.shentsize = u16::from_le_bytes(value[58..60].try_into().unwrap());
        hdr.shnum = u16::from_le_bytes(value[60..62].try_into().unwrap());
        hdr.shstrndx = u16::from_le_bytes(value[62..64].try_into().unwrap());

        Ok(hdr)
    }
}

#[repr(C)]
struct Elf64ProgramHeader {
    r#type: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

impl From<&[u8; 56]> for Elf64ProgramHeader {
    fn from(value: &[u8; 56]) -> Self {
        Self {
            r#type: u32::from_le_bytes(value[0..4].try_into().unwrap()),
            flags: u32::from_le_bytes(value[4..8].try_into().unwrap()),
            offset: u64::from_le_bytes(value[8..16].try_into().unwrap()),
            vaddr: u64::from_le_bytes(value[16..24].try_into().unwrap()),
            paddr: u64::from_le_bytes(value[24..32].try_into().unwrap()),
            filesz: u64::from_le_bytes(value[32..40].try_into().unwrap()),
            memsz: u64::from_le_bytes(value[40..48].try_into().unwrap()),
            align: u64::from_le_bytes(value[48..56].try_into().unwrap()),
        }
    }
}

impl ElfFile {
    pub fn parse(data: &[u8]) -> Result<Self, ElfError> {
        if data.len() < size_of::<Elf64Header>() {
            return Err(ElfError::Truncated);
        }
        let mut hdr_buf = [0u8; size_of::<Elf64Header>()];
        hdr_buf.copy_from_slice(&data[..size_of::<Elf64Header>()]);
        let header = Elf64Header::try_from(&hdr_buf)?;

        if header.phentsize as usize != size_of::<Elf64ProgramHeader>() {
            return Err(ElfError::UnsupportedVersion);
        }

        let mut segments = Vec::new();
        let phoff = header.phoff as usize;
        let phentsize = header.phentsize as usize;
        let phcount = header.phnum as usize;

        if phoff + phcount * phentsize > data.len() {
            return Err(ElfError::Truncated);
        }

        for idx in 0..phcount {
            let start = phoff + idx * phentsize;
            let end = start + phentsize;
            let mut buf = [0u8; size_of::<Elf64ProgramHeader>()];
            buf.copy_from_slice(&data[start..end]);
            let ph = Elf64ProgramHeader::from(&buf);
            if ph.r#type == PT_LOAD {
                segments.push(Segment {
                    vaddr: ph.vaddr,
                    mem_size: ph.memsz,
                    file_size: ph.filesz,
                    file_offset: ph.offset,
                    align: ph.align,
                    flags: ph.flags,
                });
            }
        }

        Ok(Self {
            entry: header.entry,
            segments,
            data: data.to_vec(),
        })
    }
}
