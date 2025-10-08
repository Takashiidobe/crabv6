use alloc::{string::String, vec, vec::Vec};
use core::{fmt, str};
use spin::Mutex;

use crate::virtio::block::{self, VirtIoBlock, VirtioError};

pub const BLOCK_SIZE: usize = 512;
const MAGIC: u32 = 0x5446_5331;
const VERSION: u32 = 1;
const DIR_BLOCK_INDEX: u32 = 1;
const DATA_START_BLOCK: u32 = 2;
const NAME_LEN: usize = 32;
const DIR_ENTRY_SIZE: usize = NAME_LEN + 4 + 4;
const MAX_FILES: usize = BLOCK_SIZE / DIR_ENTRY_SIZE;

static FS_INSTANCE: Mutex<Option<TinyFs<VirtIoBlock>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError {
    NotInitialized,
    NameTooLong,
    DirectoryFull,
    NotFound,
    NoSpace,
    InvalidEncoding,
    DeviceInitFailed(VirtioError),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            FsError::NotInitialized => "filesystem not initialized",
            FsError::NameTooLong => "filename too long",
            FsError::DirectoryFull => "no free directory entries",
            FsError::NotFound => "file not found",
            FsError::NoSpace => "disk is full",
            FsError::InvalidEncoding => "invalid string encoding",
            FsError::DeviceInitFailed(err) => match *err {
                VirtioError::DeviceNotFound => "virtio block device missing",
                VirtioError::UnsupportedDevice => "unsupported virtio block device",
                VirtioError::LegacyOnly(version) => {
                    if version == 1 {
                        "legacy virtio-mmio (version 1) not supported; add ,disable-legacy=on"
                    } else {
                        "unknown virtio-mmio version"
                    }
                }
                VirtioError::QueueUnavailable => "virtio queue unavailable",
                VirtioError::DeviceRejectedFeatures => "virtio feature negotiation failed",
                VirtioError::DeviceFailure => "virtio block device failed",
            },
        };
        f.write_str(message)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Superblock {
    magic: u32,
    version: u32,
    next_free_block: u32,
    file_count: u32,
}

#[derive(Clone, Debug)]
struct FileEntry {
    name: String,
    start_block: u32,
    length: u32,
}

pub trait BlockDevice {
    fn total_blocks(&self) -> u32;
    fn read_block(&self, index: u32, buf: &mut [u8]);
    fn write_block(&self, index: u32, buf: &[u8]);
}

impl BlockDevice for VirtIoBlock {
    fn total_blocks(&self) -> u32 {
        VirtIoBlock::total_blocks(self)
    }

    fn read_block(&self, index: u32, buf: &mut [u8]) {
        VirtIoBlock::read_block(self, index, buf);
    }

    fn write_block(&self, index: u32, buf: &[u8]) {
        VirtIoBlock::write_block(self, index, buf);
    }
}

pub struct TinyFs<D: BlockDevice> {
    device: D,
    superblock: Superblock,
    directory: Vec<FileEntry>,
}

impl<D: BlockDevice> TinyFs<D> {
    pub fn mount(device: D) -> Self {
        let mut fs = Self {
            superblock: Superblock::default(),
            device,
            directory: Vec::new(),
        };
        fs.load_or_format();
        fs
    }

    fn load_or_format(&mut self) {
        let mut buf = [0u8; BLOCK_SIZE];
        self.device.read_block(0, &mut buf);
        let superblock = Self::parse_superblock(&buf);
        if superblock.magic != MAGIC || superblock.version != VERSION {
            self.format_disk();
        } else {
            self.superblock = superblock;
            self.load_directory();
        }
    }

    fn format_disk(&mut self) {
        let blank = [0u8; BLOCK_SIZE];
        for block in 0..DATA_START_BLOCK {
            self.device.write_block(block, &blank);
        }
        self.superblock = Superblock {
            magic: MAGIC,
            version: VERSION,
            next_free_block: DATA_START_BLOCK,
            file_count: 0,
        };
        self.directory.clear();
        self.flush_superblock();
        self.flush_directory();
    }

    fn load_directory(&mut self) {
        self.directory.clear();
        let mut buf = [0u8; BLOCK_SIZE];
        self.device.read_block(DIR_BLOCK_INDEX, &mut buf);
        for chunk in buf.chunks(DIR_ENTRY_SIZE).take(MAX_FILES) {
            if chunk[0] == 0 {
                continue;
            }
            let name_bytes = &chunk[..NAME_LEN];
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
            let name_slice = &name_bytes[..end];
            let Ok(name_str) = str::from_utf8(name_slice) else {
                continue;
            };
            let start_block = u32::from_le_bytes(chunk[NAME_LEN..NAME_LEN + 4].try_into().unwrap());
            let length = u32::from_le_bytes(chunk[NAME_LEN + 4..NAME_LEN + 8].try_into().unwrap());
            self.directory.push(FileEntry {
                name: String::from(name_str),
                start_block,
                length,
            });
        }
    }

    fn flush_superblock(&mut self) {
        let mut buf = [0u8; BLOCK_SIZE];
        buf[..4].copy_from_slice(&self.superblock.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.superblock.version.to_le_bytes());
        buf[8..12].copy_from_slice(&self.superblock.next_free_block.to_le_bytes());
        buf[12..16].copy_from_slice(&self.superblock.file_count.to_le_bytes());
        self.device.write_block(0, &buf);
    }

    fn flush_directory(&mut self) {
        let mut buf = [0u8; BLOCK_SIZE];
        for (slot, entry) in self.directory.iter().enumerate().take(MAX_FILES) {
            let offset = slot * DIR_ENTRY_SIZE;
            let name_bytes = entry.name.as_bytes();
            let copy_len = NAME_LEN.min(name_bytes.len());
            buf[offset..offset + copy_len].copy_from_slice(&name_bytes[..copy_len]);
            buf[offset + NAME_LEN..offset + NAME_LEN + 4]
                .copy_from_slice(&entry.start_block.to_le_bytes());
            buf[offset + NAME_LEN + 4..offset + NAME_LEN + 8]
                .copy_from_slice(&entry.length.to_le_bytes());
        }
        self.device.write_block(DIR_BLOCK_INDEX, &buf);
    }

    fn parse_superblock(buf: &[u8]) -> Superblock {
        Superblock {
            magic: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            version: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
            next_free_block: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            file_count: u32::from_le_bytes(buf[12..16].try_into().unwrap()),
        }
    }

    fn allocate_blocks(&mut self, blocks: u32) -> Result<u32, FsError> {
        let start = self.superblock.next_free_block;
        if start + blocks > self.device.total_blocks() {
            return Err(FsError::NoSpace);
        }
        self.superblock.next_free_block += blocks;
        Ok(start)
    }

    fn ensure_directory_slot(&mut self, name: &str) -> Result<(), FsError> {
        if self.directory.len() < MAX_FILES {
            return Ok(());
        }
        if self
            .directory
            .iter()
            .any(|entry| entry.name.as_str() == name)
        {
            return Ok(());
        }
        Err(FsError::DirectoryFull)
    }

    pub fn list(&self) -> Vec<String> {
        self.directory
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    pub fn read_file(&self, name: &str) -> Result<Vec<u8>, FsError> {
        let Some(entry) = self.directory.iter().find(|e| e.name == name) else {
            return Err(FsError::NotFound);
        };
        let mut remaining = entry.length as usize;
        let mut data = Vec::with_capacity(remaining);
        let mut block_index = entry.start_block;
        let mut buf = vec![0u8; BLOCK_SIZE];
        while remaining > 0 {
            self.device.read_block(block_index, &mut buf);
            let take = remaining.min(BLOCK_SIZE);
            data.extend_from_slice(&buf[..take]);
            remaining -= take;
            block_index += 1;
        }
        Ok(data)
    }

    pub fn write_file(&mut self, name: &str, contents: &[u8]) -> Result<(), FsError> {
        if name.is_empty() || name.len() > NAME_LEN {
            return Err(FsError::NameTooLong);
        }
        self.ensure_directory_slot(name)?;

        let blocks_needed = contents.len().div_ceil(BLOCK_SIZE) as u32;
        let start_block = self.allocate_blocks(blocks_needed)?;

        let mut buf = [0u8; BLOCK_SIZE];
        for (i, chunk) in contents.chunks(BLOCK_SIZE).enumerate() {
            buf.fill(0);
            buf[..chunk.len()].copy_from_slice(chunk);
            self.device.write_block(start_block + i as u32, &buf);
        }

        match self
            .directory
            .iter_mut()
            .find(|entry| entry.name.as_str() == name)
        {
            Some(entry) => {
                entry.start_block = start_block;
                entry.length = contents.len() as u32;
            }
            None => {
                self.directory.push(FileEntry {
                    name: String::from(name),
                    start_block,
                    length: contents.len() as u32,
                });
                self.superblock.file_count = self.directory.len() as u32;
            }
        }

        self.flush_superblock();
        self.flush_directory();
        Ok(())
    }

    pub fn format(&mut self) {
        self.format_disk();
    }
}

pub fn init() -> Result<(), FsError> {
    let mut guard = FS_INSTANCE.lock();
    if guard.is_none() {
        let device = block::init().map_err(FsError::DeviceInitFailed)?;
        *guard = Some(TinyFs::mount(device));
    }
    Ok(())
}

fn with_fs<T>(
    f: impl FnOnce(&mut TinyFs<VirtIoBlock>) -> Result<T, FsError>,
) -> Result<T, FsError> {
    let mut guard = FS_INSTANCE.lock();
    match guard.as_mut() {
        Some(fs) => f(fs),
        None => Err(FsError::NotInitialized),
    }
}

pub fn list_files() -> Result<Vec<String>, FsError> {
    with_fs(|fs| Ok(fs.list()))
}

pub fn read_file(name: &str) -> Result<Vec<u8>, FsError> {
    with_fs(|fs| fs.read_file(name))
}

pub fn write_file(name: &str, data: &[u8]) -> Result<(), FsError> {
    with_fs(|fs| fs.write_file(name, data))
}

pub fn format() -> Result<(), FsError> {
    with_fs(|fs| {
        fs.format();
        Ok(())
    })
}
