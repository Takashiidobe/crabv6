# TinyFS Filesystem

TinyFS is a simple, custom filesystem implementation designed for educational purposes in Crabv6. It provides hierarchical directories and basic file operations.

## Overview

**Implementation**: `src/fs.rs`

**Design Goals**:
- Simple to understand and implement
- Hierarchical directory structure
- Block-based storage on VirtIO device
- Sufficient for teaching OS filesystem concepts

**Characteristics**:
- Block size: 512 bytes
- Maximum file size: Limited by available blocks
- Directory structure: Hierarchical with unlimited depth
- Allocation: Simple bump allocator (no free list)

## On-Disk Layout

### Block Allocation

```
Block 0: Superblock
Block 1: Root directory
Block 2+: Data blocks (files and subdirectories)
```

Total disk size: 16MB (32768 blocks of 512 bytes each)

### Superblock Format

**Location**: Block 0

**Structure** (`src/fs.rs:15-20`):
```rust
struct Superblock {
    magic: u32,           // 0x54465320 ("TFS ")
    version: u32,         // 1
    next_free_block: u32, // Next block to allocate
    file_count: u32,      // Total number of files/dirs
}
```

**Size**: 16 bytes (remainder of block unused)

**Initialization**:
```rust
Superblock {
    magic: 0x54465320,     // Magic number for validation
    version: 1,             // Version 1
    next_free_block: 2,     // Start allocating at block 2
    file_count: 0,          // No files initially
}
```

### Directory Format

**Root Directory**: Block 1

**Structure**: Array of directory entries

**Directory Entry** (`src/fs.rs:22-30`):
```rust
pub struct DirEntry {
    pub name: [u8; 32],        // Null-terminated filename
    pub block_or_size: u32,    // Block number (dir) or size (file)
    pub is_dir: bool,          // true = directory, false = file
    pub _padding: [u8; 3],     // Alignment padding
}
```

**Size**: 40 bytes per entry

**Entries per Block**: 512 / 40 = 12 entries (with 32 bytes remainder)

**Root Directory Limit**: 11 entries
- Entry 0 is reserved for parent directory reference
- Entries 1-11 are usable (11 files/dirs in root)

### File Storage

Files are stored as raw data in allocated blocks.

**Single Block File** (<= 512 bytes):
```
File entry: { name, size, is_dir=false }
Data: Stored directly in one block
```

**Multi-Block File** (> 512 bytes):
Currently NOT supported. Files are limited to single block (512 bytes).

**Future**: Could implement indirect blocks or extent lists.

### Subdirectory Storage

Subdirectories are stored as serialized arrays of directory entries.

**Subdirectory Block**:
```
Block N:
  Entry 0: { name: "..", block_or_size: parent_block, is_dir: true }
  Entry 1-11: Child files and directories
```

**Depth**: Unlimited (limited only by disk space)

## Filesystem Operations

### Format

**Function**: `TinyFS::format()` (`src/fs.rs:180-200`)

**Process**:
1. Create superblock with magic, version=1, next_free_block=2
2. Write superblock to block 0
3. Create empty root directory (all zero entries)
4. Write root directory to block 1

**Usage**:
```
/> fs format
Filesystem formatted successfully
```

**Effect**: Erases all existing data and creates a clean filesystem.

### Read File

**Function**: `TinyFS::read_file(path)` (`src/fs.fs:248-275`)

**Process**:
1. Resolve path to directory entry
2. Verify entry is a file (not directory)
3. Read file size from entry
4. Allocate buffer of size bytes
5. Read data from block specified in entry
6. Return data as `Vec<u8>`

**Example**:
```rust
let data = fs.read_file("/test/hello.txt")?;
println!("{}", String::from_utf8_lossy(&data));
```

**Errors**:
- `ENOENT` - File not found
- `EINVAL` - Path is a directory, not a file

### Write File

**Function**: `TinyFS::write_file(path, data)` (`src/fs.rs:277-320`)

**Process**:
1. Split path into directory and filename
2. Navigate to parent directory
3. Check if file already exists:
   - If exists: Overwrite (allocate new block, old block leaked)
   - If not exists: Create new entry
4. Allocate new data block
5. Write data to block
6. Update directory entry with new block and size
7. Update superblock

**Example**:
```rust
fs.write_file("/test/hello.txt", b"Hello, world!")?;
```

**Limitations**:
- File size limited to 512 bytes (single block)
- Overwriting a file leaks the old block (no free list)
- No append mode (entire file replaced)

**Errors**:
- `ENOENT` - Parent directory not found
- `ENOSPC` - No free blocks or directory full

### Create Directory

**Function**: `TinyFS::create_dir(path)` (`src/fs.rs:322-370`)

**Process**:
1. Split path into parent and directory name
2. Navigate to parent directory
3. Check directory doesn't already exist
4. Allocate block for new directory
5. Initialize directory with ".." entry pointing to parent
6. Write directory to block
7. Add entry to parent directory
8. Update superblock

**Example**:
```rust
fs.create_dir("/test")?;
fs.create_dir("/test/subdir")?;
```

**Directory Structure**:
```
/test/subdir:
  Entry 0: { name: "..", block: (block of /test), is_dir: true }
  Entry 1-11: (empty, available for files)
```

**Errors**:
- `ENOENT` - Parent directory not found
- `EINVAL` - Directory already exists
- `ENOSPC` - No free blocks or parent directory full

### Delete File

**Function**: `TinyFS::delete_file(path)` (`src/fs.rs:372-400`)

**Process**:
1. Resolve path to parent directory
2. Find file entry in parent
3. Verify entry is a file (not directory)
4. Remove entry from parent directory
5. Shift remaining entries down
6. Write updated parent directory

**Example**:
```rust
fs.delete_file("/test/hello.txt")?;
```

**Limitations**:
- File's data block is leaked (no free list)
- Cannot delete directories (use delete_dir)

**Errors**:
- `ENOENT` - File not found
- `EINVAL` - Path is a directory

### Delete Directory

**Function**: `TinyFS::delete_dir(path)` (`src/fs.rs:402-440`)

**Process**:
1. Resolve path to directory entry
2. Verify directory is empty (only ".." entry)
3. Remove entry from parent directory
4. Update parent directory

**Example**:
```rust
fs.delete_dir("/test/empty_dir")?;
```

**Errors**:
- `ENOENT` - Directory not found
- `ENOTEMPTY` - Directory contains files/subdirs
- `EINVAL` - Cannot delete root directory

### List Directory

**Function**: `TinyFS::list_dir(path)` (`src/fs.rs:442-475`)

**Process**:
1. Resolve path to directory entry
2. Read directory block
3. Parse entries
4. Return vector of entries (excluding "..")

**Example**:
```rust
let entries = fs.list_dir("/test")?;
for entry in entries {
    println!("{} - {}",
        String::from_utf8_lossy(&entry.name),
        if entry.is_dir { "DIR" } else { "FILE" }
    );
}
```

**Returns**: `Vec<DirEntry>` with all entries except ".."

## Path Resolution

### Path Parsing

**Function**: `TinyFS::resolve_path(path)` (`src/fs.rs:522-600`)

**Supported Paths**:
- Absolute: `/test/file.txt`
- Relative to CWD: `file.txt`, `subdir/file.txt`
- Parent: `..`
- Current: `.`

**Algorithm**:
1. Start from root or current working directory
2. Split path on `/` separator
3. For each component:
   - If `.`: continue (no-op)
   - If `..`: move to parent directory
   - Otherwise: search for entry in current directory
4. Return final directory block and entry name

**Example**:
```
CWD: /test
Path: ../other/file.txt

Steps:
1. Start at /test (block 2)
2. ".." -> move to / (block 1)
3. "other" -> find in / (block 3)
4. Return: (block 3, "file.txt")
```

### Current Working Directory

**Storage**: `TinyFS.current_dir` (String)

**Operations**:
- `change_dir(path)` - Change CWD (`src/fs.rs:477-520`)
- Stored as absolute path (e.g., `/test/subdir`)
- Root is `/`

**Validation**:
- Path must exist
- Path must be a directory

## Block I/O

### VirtIO Integration

**Driver**: `src/virtio.rs`

**Interface**:
```rust
fn read_block(&mut self, block_num: u32, buf: &mut [u8; 512])
fn write_block(&mut self, block_num: u32, buf: &[u8; 512])
```

**Process**:
1. Set up virtqueue descriptor
2. Notify device
3. Poll for completion
4. Read result

**Synchronous**: All I/O operations block until complete.

### Caching

**Current**: No caching. Every operation reads/writes blocks directly.

**Future**: Could implement:
- Directory entry cache
- Block cache
- Write-back buffering

## Concurrency

### Locking

**Global Mutex**: `static FS: Mutex<TinyFS>`

**Access**:
```rust
let mut fs = FS.lock();
fs.read_file("/test/file.txt")?;
```

**Behavior**:
- All filesystem operations are serialized
- Lock held for entire operation
- No concurrent access

**Trade-offs**:
- ✅ Simple, no race conditions
- ❌ No parallelism
- ❌ Blocks all access during I/O

## Error Handling

### Error Codes

Defined in `src/syscall.rs:52-61`:

```rust
ENOENT   = -2   // No such file or directory
EINVAL   = -22  // Invalid argument
ENOSPC   = -28  // No space left on device
ENOTEMPTY = -39 // Directory not empty
```

### Error Propagation

Filesystem functions return `Result<T, FsError>`:

```rust
pub enum FsError {
    NotFound,        // -> ENOENT
    InvalidArg,      // -> EINVAL
    NoSpace,         // -> ENOSPC
    DirNotEmpty,     // -> ENOTEMPTY
    VirtioError(VirtioError),
}
```

Syscall layer converts to errno codes.

## Design Decisions

### Choice: Bump Allocator (No Free List)

**Current**:
```rust
let block = self.superblock.next_free_block;
self.superblock.next_free_block += 1;
```

**Rationale**:
- Simple to implement
- No fragmentation (blocks allocated sequentially)
- Sufficient for teaching purposes

**Trade-offs**:
- ✅ Simple, no free list management
- ❌ No space reclamation (deleted files leak blocks)
- ❌ Disk fills up eventually even with deletes

**Future**: Could implement bitmap or linked free list.

### Choice: Single-Block Files

**Current**: Files limited to 512 bytes.

**Rationale**:
- Simplifies implementation
- No indirect blocks needed
- Sufficient for small config files and test data

**Future**: Could implement:
- Direct + indirect blocks (like Unix inode)
- Extent lists
- File allocation table (FAT-style)

### Choice: Root Directory Size Limit

**Current**: 11 entries maximum in root directory.

**Rationale**:
- Root stored in single block (block 1)
- 12 entries per block, entry 0 reserved
- Subdirectories have same limit per directory

**Future**: Could implement:
- Multiple blocks per directory
- B-tree directory structure
- Hash-based lookup

### Choice: No Journaling

**Current**: Direct writes, no transaction log.

**Implications**:
- Crash during write can corrupt filesystem
- No atomic operations
- No recovery mechanism

**Rationale**:
- Journaling adds significant complexity
- Teaching OS doesn't require crash resilience
- QEMU environment is stable

## Limitations

Current limitations of TinyFS:

1. **Space Management**:
   - No free list (deleted blocks are leaked)
   - No defragmentation
   - Disk fills up and can't be reclaimed

2. **File Size**:
   - Single block (512 bytes) maximum
   - No large file support

3. **Directory Size**:
   - 11 entries per directory
   - No overflow handling

4. **Performance**:
   - No caching (every operation hits disk)
   - Synchronous I/O (blocks kernel)
   - No read-ahead or write buffering

5. **Reliability**:
   - No journaling
   - No checksums or error detection
   - Crash can corrupt filesystem

6. **Features**:
   - No permissions or ownership
   - No timestamps
   - No symbolic links
   - No file attributes

## Usage Examples

### Create Hierarchical Structure

```
/> fs format
/> fs mkdir /home
/> fs mkdir /home/user
/> fs write /home/user/config.txt "setting=value"
/> fs cd /home/user
/home/user> fs cat config.txt
setting=value
```

### File Operations

```
/> fs write /test.txt "Hello, world!"
/> fs cat /test.txt
Hello, world!

/> fs write /test.txt "Updated content"
/> fs cat /test.txt
Updated content

/> fs rm /test.txt
/> fs cat /test.txt
Error: No such file or directory
```

### Directory Navigation

```
/> fs mkdir /a
/> fs mkdir /a/b
/> fs mkdir /a/b/c
/> fs cd /a/b/c
/a/b/c> fs cd ../..
/a> fs cd /
/>
```

## Future Enhancements

Potential improvements to TinyFS:

1. **Free Space Management**:
   - [ ] Implement bitmap allocator
   - [ ] Add space reclamation on delete
   - [ ] Track free/used blocks in superblock

2. **Large File Support**:
   - [ ] Indirect blocks (single, double, triple)
   - [ ] Extent-based allocation
   - [ ] Maximum file size in GB range

3. **Performance**:
   - [ ] Block cache (LRU)
   - [ ] Directory entry cache
   - [ ] Write-back buffering
   - [ ] Asynchronous I/O

4. **Reliability**:
   - [ ] Journaling (metadata or full)
   - [ ] Block checksums
   - [ ] Filesystem check and repair (fsck)
   - [ ] Backup superblock

5. **Features**:
   - [ ] File permissions (rwx for user/group/other)
   - [ ] Timestamps (created, modified, accessed)
   - [ ] Symbolic and hard links
   - [ ] Extended attributes

6. **Scalability**:
   - [ ] Larger directories (B-tree or hash table)
   - [ ] Larger filesystem (64-bit block numbers)
   - [ ] Multiple disks/partitions

---

## Summary

TinyFS provides a simple, functional filesystem that demonstrates:
- Block-based storage
- Hierarchical directory structure
- Path resolution
- File and directory operations
- Integration with block device driver

While limited in features and scalability, it serves as an excellent educational example of filesystem fundamentals and is sufficient for the teaching goals of Crabv6.
