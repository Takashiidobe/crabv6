use alloc::{string::String, vec, vec::Vec};
use core::{fmt, str};
use spin::Mutex;

use crate::virtio::block::{self, VirtIoBlock, VirtioError};

pub const BLOCK_SIZE: usize = 512;
const MAGIC: u32 = 0x5446_5331;
const VERSION: u32 = 2;
const DIR_BLOCK_INDEX: u32 = 1;
const DATA_START_BLOCK: u32 = 2;
const NAME_LEN: usize = 32;
const DIR_ENTRY_SIZE: usize = NAME_LEN + 4 + 4 + 1 + 3;
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
    InvalidPath,
    NotADirectory,
    AlreadyExists,
    DirectoryNotEmpty,
    IsDirectory,
    IsFile,
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
            FsError::InvalidPath => "invalid path",
            FsError::NotADirectory => "not a directory",
            FsError::AlreadyExists => "entry already exists",
            FsError::DirectoryNotEmpty => "directory not empty",
            FsError::IsDirectory => "expected file but found directory",
            FsError::IsFile => "expected directory but found file",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryType {
    File = 1,
    Directory = 2,
}

impl EntryType {
    fn from_raw(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::File),
            2 => Some(Self::Directory),
            _ => None,
        }
    }

    fn to_raw(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Debug)]
struct FileEntry {
    name: String,
    start_block: u32,
    length: u32,
    kind: EntryType,
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

struct TinyFs<D: BlockDevice> {
    device: D,
    superblock: Superblock,
    root_entries: Vec<FileEntry>,
}

impl<D: BlockDevice> TinyFs<D> {
    pub fn mount(device: D) -> Self {
        let mut fs = Self {
            superblock: Superblock::default(),
            device,
            root_entries: Vec::new(),
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
            self.load_root_directory();
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
        self.root_entries.clear();
        self.flush_root_directory();
        self.flush_superblock();
    }

    fn load_root_directory(&mut self) {
        self.root_entries.clear();
        let mut buf = [0u8; BLOCK_SIZE];
        self.device.read_block(DIR_BLOCK_INDEX, &mut buf);
        for chunk in buf.chunks(DIR_ENTRY_SIZE).take(MAX_FILES) {
            if let Some(entry) = deserialize_entry(chunk) {
                self.root_entries.push(entry);
            }
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

    fn flush_root_directory(&mut self) {
        let mut buf = [0u8; BLOCK_SIZE];
        for (slot, entry) in self.root_entries.iter().enumerate().take(MAX_FILES) {
            let offset = slot * DIR_ENTRY_SIZE;
            write_entry(&mut buf[offset..offset + DIR_ENTRY_SIZE], entry);
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

    fn allocate_and_write(&mut self, contents: &[u8]) -> Result<(u32, u32), FsError> {
        if contents.is_empty() {
            return Ok((0, 0));
        }
        let blocks_needed = contents.len().div_ceil(BLOCK_SIZE) as u32;
        let start_block = self.allocate_blocks(blocks_needed)?;
        let mut buf = [0u8; BLOCK_SIZE];
        for (i, chunk) in contents.chunks(BLOCK_SIZE).enumerate() {
            buf.fill(0);
            buf[..chunk.len()].copy_from_slice(chunk);
            self.device.write_block(start_block + i as u32, &buf);
        }
        Ok((start_block, contents.len() as u32))
    }

    fn read_data(&self, start_block: u32, length: u32) -> Vec<u8> {
        if length == 0 {
            return Vec::new();
        }
        let mut remaining = length as usize;
        let mut data = Vec::with_capacity(remaining);
        let mut block_index = start_block;
        let mut buf = vec![0u8; BLOCK_SIZE];
        while remaining > 0 {
            self.device.read_block(block_index, &mut buf);
            let take = remaining.min(BLOCK_SIZE);
            data.extend_from_slice(&buf[..take]);
            remaining -= take;
            block_index += 1;
        }
        data
    }

    fn read_directory_entries(&self, entry: &FileEntry) -> Result<Vec<FileEntry>, FsError> {
        if entry.kind != EntryType::Directory {
            return Err(FsError::NotADirectory);
        }
        if entry.length == 0 {
            return Ok(Vec::new());
        }
        let raw = self.read_data(entry.start_block, entry.length);
        let mut entries = Vec::new();
        for chunk in raw.chunks(DIR_ENTRY_SIZE) {
            if chunk.len() < DIR_ENTRY_SIZE {
                break;
            }
            if let Some(e) = deserialize_entry(chunk) {
                entries.push(e);
            }
        }
        Ok(entries)
    }

    fn write_directory_entries(&mut self, entries: &[FileEntry]) -> Result<(u32, u32), FsError> {
        if entries.is_empty() {
            return Ok((0, 0));
        }
        let mut data = vec![0u8; entries.len() * DIR_ENTRY_SIZE];
        for (i, entry) in entries.iter().enumerate() {
            let offset = i * DIR_ENTRY_SIZE;
            write_entry(&mut data[offset..offset + DIR_ENTRY_SIZE], entry);
        }
        self.allocate_and_write(&data)
    }

    fn split_path<'a>(&self, path: &'a str) -> Result<Vec<&'a str>, FsError> {
        if path.is_empty() {
            return Ok(Vec::new());
        }
        let components: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        Ok(components)
    }

    fn load_directory_chain(&mut self, components: &[&str]) -> Result<Vec<LoadedDir>, FsError> {
        let mut chain = Vec::new();
        chain.push(LoadedDir {
            entries: self.root_entries.clone(),
            entry_index_in_parent: None,
        });

        for component in components {
            if component.is_empty() {
                continue;
            }
            let current = chain.last().expect("chain always has root");
            let Some((idx, entry)) = current
                .entries
                .iter()
                .enumerate()
                .find(|(_, e)| e.name == *component)
            else {
                return Err(FsError::NotFound);
            };
            if entry.kind != EntryType::Directory {
                return Err(FsError::NotADirectory);
            }
            let child_entries = self.read_directory_entries(entry)?;
            chain.push(LoadedDir {
                entries: child_entries,
                entry_index_in_parent: Some(idx),
            });
        }

        Ok(chain)
    }

    fn persist_directory_chain(&mut self, chain: &mut [LoadedDir]) -> Result<(), FsError> {
        for level in (1..chain.len()).rev() {
            let (parents, current) = chain.split_at_mut(level);
            let parent = &mut parents[level - 1];
            let current_dir = &current[0];
            let (start, length) = self.write_directory_entries(&current_dir.entries)?;
            if let Some(idx) = current_dir.entry_index_in_parent {
                parent.entries[idx].start_block = start;
                parent.entries[idx].length = length;
            }
        }
        self.root_entries = core::mem::take(&mut chain[0].entries);
        self.superblock.file_count = self.root_entries.len() as u32;
        self.flush_root_directory();
        self.flush_superblock();
        Ok(())
    }

    fn list_directory(&mut self, path: &str) -> Result<Vec<String>, FsError> {
        let components = self.split_path(path)?;
        let chain = self.load_directory_chain(&components)?;
        let entries = &chain.last().expect("chain non-empty").entries;
        let mut names = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry.kind {
                EntryType::File => names.push(entry.name.clone()),
                EntryType::Directory => {
                    let mut name = entry.name.clone();
                    name.push('/');
                    names.push(name);
                }
            }
        }
        Ok(names)
    }

    fn read_file_contents(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
        let components = self.split_path(path)?;
        if components.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let (dirs, leaf) = components.split_at(components.len() - 1);
        let mut chain = self.load_directory_chain(dirs)?;
        let entries = chain.last_mut().expect("chain non-empty");
        let Some(entry) = entries.entries.iter().find(|entry| entry.name == leaf[0]) else {
            return Err(FsError::NotFound);
        };
        if entry.kind != EntryType::File {
            return Err(FsError::NotADirectory);
        }
        Ok(self.read_data(entry.start_block, entry.length))
    }

    fn write_file_contents(&mut self, path: &str, contents: &[u8]) -> Result<(), FsError> {
        let components = self.split_path(path)?;
        if components.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let (dirs, leaf) = components.split_at(components.len() - 1);
        let file_name = leaf[0];
        if file_name.is_empty() || file_name.len() > NAME_LEN {
            return Err(FsError::NameTooLong);
        }
        let mut chain = self.load_directory_chain(dirs)?;
        let parent_is_root = chain.len() == 1;
        let parent_entries = chain.last_mut().expect("chain non-empty");

        let existing_index = parent_entries
            .entries
            .iter()
            .position(|entry| entry.name == file_name);

        if existing_index.is_none() && parent_is_root && parent_entries.entries.len() >= MAX_FILES {
            return Err(FsError::DirectoryFull);
        }

        let (start_block, length) = self.allocate_and_write(contents)?;

        match existing_index {
            Some(idx) => {
                if parent_entries.entries[idx].kind != EntryType::File {
                    return Err(FsError::NotADirectory);
                }
                parent_entries.entries[idx].start_block = start_block;
                parent_entries.entries[idx].length = length;
            }
            None => {
                parent_entries.entries.push(FileEntry {
                    name: String::from(file_name),
                    start_block,
                    length,
                    kind: EntryType::File,
                });
            }
        }

        self.persist_directory_chain(&mut chain)
    }

    fn create_directory(&mut self, path: &str) -> Result<(), FsError> {
        let components = self.split_path(path)?;
        if components.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let (dirs, leaf) = components.split_at(components.len() - 1);
        let dir_name = leaf[0];
        if dir_name.is_empty() || dir_name.len() > NAME_LEN {
            return Err(FsError::NameTooLong);
        }
        let mut chain = self.load_directory_chain(dirs)?;
        let parent_is_root = chain.len() == 1;
        let parent_entries = chain.last_mut().expect("chain non-empty");

        if let Some(entry) = parent_entries
            .entries
            .iter()
            .find(|entry| entry.name == dir_name)
        {
            return Err(FsError::AlreadyExists);
        }

        if parent_is_root && parent_entries.entries.len() >= MAX_FILES {
            return Err(FsError::DirectoryFull);
        }

        parent_entries.entries.push(FileEntry {
            name: String::from(dir_name),
            start_block: 0,
            length: 0,
            kind: EntryType::Directory,
        });

        self.persist_directory_chain(&mut chain)
    }

    fn create_file(&mut self, path: &str) -> Result<(), FsError> {
        let components = self.split_path(path)?;
        if components.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let (dirs, leaf) = components.split_at(components.len() - 1);
        let file_name = leaf[0];
        if file_name.is_empty() || file_name.len() > NAME_LEN {
            return Err(FsError::NameTooLong);
        }

        let mut chain = self.load_directory_chain(dirs)?;
        let parent_is_root = chain.len() == 1;
        let parent_entries = chain.last_mut().expect("chain non-empty");

        if parent_entries
            .entries
            .iter()
            .any(|entry| entry.name == file_name)
        {
            return Err(FsError::AlreadyExists);
        }

        if parent_is_root && parent_entries.entries.len() >= MAX_FILES {
            return Err(FsError::DirectoryFull);
        }

        parent_entries.entries.push(FileEntry {
            name: String::from(file_name),
            start_block: 0,
            length: 0,
            kind: EntryType::File,
        });

        self.persist_directory_chain(&mut chain)
    }

    fn remove_file(&mut self, path: &str) -> Result<(), FsError> {
        let components = self.split_path(path)?;
        if components.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let (dirs, leaf) = components.split_at(components.len() - 1);
        let file_name = leaf[0];

        let mut chain = self.load_directory_chain(dirs)?;
        let parent_entries = chain.last_mut().expect("chain non-empty");

        let Some(idx) = parent_entries
            .entries
            .iter()
            .position(|entry| entry.name == file_name)
        else {
            return Err(FsError::NotFound);
        };

        if parent_entries.entries[idx].kind != EntryType::File {
            return Err(FsError::IsDirectory);
        }

        parent_entries.entries.remove(idx);
        self.persist_directory_chain(&mut chain)
    }

    fn remove_directory(&mut self, path: &str) -> Result<(), FsError> {
        let components = self.split_path(path)?;
        if components.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let (dirs, leaf) = components.split_at(components.len() - 1);
        let dir_name = leaf[0];

        let mut chain = self.load_directory_chain(dirs)?;
        let parent_entries = chain.last_mut().expect("chain non-empty");

        let Some(idx) = parent_entries
            .entries
            .iter()
            .position(|entry| entry.name == dir_name)
        else {
            return Err(FsError::NotFound);
        };

        if parent_entries.entries[idx].kind != EntryType::Directory {
            return Err(FsError::IsFile);
        }

        let entry = parent_entries.entries[idx].clone();
        let children = self.read_directory_entries(&entry)?;
        if !children.is_empty() {
            return Err(FsError::DirectoryNotEmpty);
        }

        parent_entries.entries.remove(idx);
        self.persist_directory_chain(&mut chain)
    }
}

struct LoadedDir {
    entries: Vec<FileEntry>,
    entry_index_in_parent: Option<usize>,
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

pub fn list_files(path: Option<&str>) -> Result<Vec<String>, FsError> {
    with_fs(|fs| fs.list_directory(path.unwrap_or("")))
}

pub fn read_file(path: &str) -> Result<Vec<u8>, FsError> {
    with_fs(|fs| fs.read_file_contents(path))
}

pub fn write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
    with_fs(|fs| fs.write_file_contents(path, data))
}

pub fn mkdir(path: &str) -> Result<(), FsError> {
    with_fs(|fs| fs.create_directory(path))
}

pub fn ensure_directory(path: &str) -> Result<(), FsError> {
    with_fs(|fs| fs.ensure_directory_exists(path))
}

pub fn create_file(path: &str) -> Result<(), FsError> {
    with_fs(|fs| fs.create_file(path))
}

pub fn remove_file(path: &str) -> Result<(), FsError> {
    with_fs(|fs| fs.remove_file(path))
}

pub fn remove_directory(path: &str) -> Result<(), FsError> {
    with_fs(|fs| fs.remove_directory(path))
}

pub fn format() -> Result<(), FsError> {
    with_fs(|fs| {
        fs.format_disk();
        Ok(())
    })
}

fn write_entry(buf: &mut [u8], entry: &FileEntry) {
    buf.fill(0);
    let name_bytes = entry.name.as_bytes();
    let copy_len = NAME_LEN.min(name_bytes.len());
    buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    buf[NAME_LEN..NAME_LEN + 4].copy_from_slice(&entry.start_block.to_le_bytes());
    buf[NAME_LEN + 4..NAME_LEN + 8].copy_from_slice(&entry.length.to_le_bytes());
    buf[NAME_LEN + 8] = entry.kind.to_raw();
}

fn deserialize_entry(chunk: &[u8]) -> Option<FileEntry> {
    if chunk.len() < DIR_ENTRY_SIZE {
        return None;
    }
    if chunk[0] == 0 {
        return None;
    }
    let name_bytes = &chunk[..NAME_LEN];
    let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    let name_slice = &name_bytes[..end];
    let name = match str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let start_block = u32::from_le_bytes(chunk[NAME_LEN..NAME_LEN + 4].try_into().unwrap());
    let length = u32::from_le_bytes(chunk[NAME_LEN + 4..NAME_LEN + 8].try_into().unwrap());
    let kind = EntryType::from_raw(chunk[NAME_LEN + 8])?;
    Some(FileEntry {
        name: String::from(name),
        start_block,
        length,
        kind,
    })
}

impl<D: BlockDevice> TinyFs<D> {
    fn ensure_directory_exists(&mut self, path: &str) -> Result<(), FsError> {
        let components = self.split_path(path)?;
        self.load_directory_chain(&components)?;
        Ok(())
    }
}
