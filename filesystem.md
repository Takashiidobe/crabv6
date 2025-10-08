# TinyFS Overview

The kernel ships with a deliberately small filesystem (`src/fs.rs`) that
sits directly on top of the VirtIO-MMIO block driver (`src/virtio.rs`).
This document walks through the layout, how hierarchical directories are
implemented, and the tradeoffs that keep the code compact.

## On-disk layout

TinyFS fixes the block size at 512 bytes and reserves the first two
blocks for metadata:

```rust
pub const BLOCK_SIZE: usize = 512;
const DIR_BLOCK_INDEX: u32 = 1;
const DATA_START_BLOCK: u32 = 2;
```

- **Block 0 – Superblock.** Holds a magic value, on-disk format version,
  the next free block (`next_free_block`), and a cached count of root
  entries. A mismatched magic or version forces a fresh format.
- **Block 1 – Root directory table.** Split into fixed-width entries. An
  entry stores a `name[32]`, `start_block`, `length`, and a one-byte
  `EntryType` (`1 = file`, `2 = directory`). Only the root directory is
  limited to the block-size constraint (`MAX_FILES = 11`).
- **Blocks 2+ – Payload storage.** Regular files store their contents
  here. Directories deeper than the root also live here: their entries
  are serialized into a byte array and written like an ordinary file.

Formatting zeroes the metadata blocks and seeds the allocator:

```rust
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
```

## Directory handling

The root directory is cached in memory; each slot is decoded using the
entry helpers at the bottom of `fs.rs`:

```rust
fn deserialize_entry(chunk: &[u8]) -> Option<FileEntry> {
    if chunk.len() < DIR_ENTRY_SIZE || chunk[0] == 0 {
        return None;
    }
    let name_bytes = &chunk[..NAME_LEN];
    let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    let name = str::from_utf8(&name_bytes[..end]).ok()?;
    let start_block = u32::from_le_bytes(chunk[NAME_LEN..NAME_LEN + 4].try_into().unwrap());
    let length = u32::from_le_bytes(chunk[NAME_LEN + 4..NAME_LEN + 8].try_into().unwrap());
    let kind = EntryType::from_raw(chunk[NAME_LEN + 8])?;
    Some(FileEntry { name: String::from(name), start_block, length, kind })
}
```

Nested directories are materialised on demand. When walking a path the
filesystem builds a `Vec<LoadedDir>` that mirrors the directory chain:

```rust
let mut chain = self.load_directory_chain(&components)?; // root + every directory
let parent_entries = chain.last_mut().expect("chain non-empty");
```

Each directory snapshot in the chain owns a `Vec<FileEntry>`. After a
mutation (creating a file, adding a directory, etc.) TinyFS walks the
chain from leaf to root, serialising each directory back to disk and
propagating the new `start_block`/`length` into its parent:

```rust
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
self.flush_root_directory();
self.flush_superblock();
```

Root entries are limited by the single metadata block (`MAX_FILES`), but
subdirectories can grow past a block because their serialized entries
are written into the data area like any other file.

## File IO

Files are written as contiguous extents. Blocks are allocated by bumping
`next_free_block`, zeroed, and filled chunk by chunk:

```rust
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
```

Reads reverse the process, streaming blocks until the stored byte length
is satisfied and trimming any padding:

```rust
let mut remaining = entry.length as usize;
let mut block_index = entry.start_block;
let mut buf = vec![0u8; BLOCK_SIZE];
while remaining > 0 {
    self.device.read_block(block_index, &mut buf);
    let take = remaining.min(BLOCK_SIZE);
    data.extend_from_slice(&buf[..take]);
    remaining -= take;
    block_index += 1;
}
```

Because there is no free-list, overwriting a file or directory allocates
fresh blocks and leaks the old extents.

## Public interface

`fs::init` brings up the VirtIO block device and mounts TinyFS once. All
helpers (`list_files`, `write_file`, `read_file`, `mkdir`, `ensure_directory`, `format`) use
`with_fs` to lock the global instance behind a `spin::Mutex`.

- `list_files(Some("path/to/dir"))` returns names, appending `/` for
  directory entries.
- `write_file("path/to/file", data)` creates or overwrites a file,
  allocating new blocks for the payload.
- `mkdir("path/to/dir")` creates empty directories (they acquire blocks
  only when entries are added).
- `ensure_directory("path/to/dir")` validates a directory path without
  mutating the filesystem (used by the shell's `cd`).

The shell (`src/main.rs`) wires these up via `fs ls`, `fs cat`, `fs
write`, `fs mkdir`, `fs cd`, and `fs format` commands.

## VirtIO-MMIO driver recap

The driver in `src/virtio.rs` negotiates the VirtIO 1.0 MMIO interface
at `0x1000_1000`, sets up an 8-element queue, and issues single
three-descriptor requests (header → data → status). It busy-polls the
used ring:

```rust
if ptr::read_volatile(ptr::addr_of!(VIRTQ_USED.idx)) == expected {
    break;
}
spin_loop();
```

Legacy (version 1) devices are rejected early
(`VirtioError::LegacyOnly(version)`) so the shell can hint to boot QEMU
with `-global virtio-mmio.force-legacy=off`.

## Tradeoffs and limitations

- **No free-space reclamation.** Every rewrite advances the bump
  allocator; unreachable extents accumulate until the disk is full.
- **Root directory cap.** Only the root is limited to `MAX_FILES`
  entries because it lives in the fixed metadata block. Subdirectories
  can grow arbitrarily by consuming data blocks, but the root remains a
  bottleneck.
- **Whole-block metadata writes.** Root updates rewrite the entire 512
  byte block. Nested directories are rewritten wholesale as well.
- **Global locking.** A single `spin::Mutex` serialises all filesystem
  operations. Fine for the current single-shell environment, but it
  blocks concurrency.
- **Busy-waiting driver.** The VirtIO layer polls for completion and only
  allows one in-flight request. It wastes CPU and ignores interrupt-driven
  completion paths.
- **Format-on-mismatch.** TinyFS still reformats on any version mismatch,
  so upgrades destroy prior contents.

These compromises keep the implementation approachable for bring-up, but
real workloads would need free-space tracking, safer metadata updates,
more capable directory management, and an interrupt-driven block driver.
