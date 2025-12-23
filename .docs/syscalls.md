# System Calls

This document describes the system call interface between user-space programs and the Crabv6 kernel.

## Overview

System calls provide the mechanism for user programs to request services from the kernel. In Crabv6, syscalls are made using the RISC-V `ecall` instruction, which traps to supervisor mode.

**Implementation**: `src/syscall.rs`, `src/kernel_entry.S`

## Calling Convention

### RISC-V ABI

Syscalls follow a custom calling convention:

**Registers**:
- `a0` (x10): Syscall number
- `a1-a6` (x11-x16): Arguments (up to 6 arguments)
- `a0` (x10): Return value (after `ecall` returns)

**Example** (from user space):
```rust
let ret: isize;
unsafe {
    asm!(
        "ecall",
        in("a0") syscall_number,
        in("a1") arg0,
        in("a2") arg1,
        lateout("a0") ret,
    );
}
```

### Return Values

**Success**: Non-negative values (>= 0)
- For read operations: Number of bytes read
- For write operations: Number of bytes written
- For other operations: 0 or specific success code

**Errors**: Negative values (< 0)
- Error codes follow Unix errno convention
- Defined in `src/syscall.rs:52-61`

### Error Codes

```rust
pub const ENOENT: isize = -2;     // No such file or directory
pub const EINVAL: isize = -22;    // Invalid argument
pub const ENOSPC: isize = -28;    // No space left on device
pub const ENOTEMPTY: isize = -39; // Directory not empty
```

## Syscall Dispatch

### Trap Handler Flow

1. **User executes `ecall`**
2. **Trap to kernel** (`src/kernel_entry.S:1-69`)
   - Save all registers to kernel stack
   - Identify trap as syscall (check `scause`)
3. **Call dispatcher** (`src/kernel_entry.S:70-81`)
   ```asm
   call syscall_handler  # Arguments already in a0-a6
   ```
4. **Dispatcher** (`src/syscall.rs:14-50`)
   - Match on `a0` (syscall number)
   - Call appropriate handler
   - Return result in `a0`
5. **Return to user** (`src/kernel_entry.S:82-106`)
   - Restore registers (except `a0` = return value)
   - Execute `sret` to return to U-mode

### Dispatcher Implementation

**Function**: `syscall_handler` (`src/syscall.rs:14-50`)

```rust
#[no_mangle]
pub extern "C" fn syscall_handler(
    syscall_num: usize, a1: usize, a2: usize, a3: usize,
    a4: usize, a5: usize, a6: usize,
) -> isize {
    match syscall_num {
        SYS_WRITE => sys_write(a1, a2, a3),
        SYS_EXIT => sys_exit(a1),
        SYS_FILE_WRITE => sys_file_write(a1, a2, a3, a4),
        SYS_FILE_READ => sys_file_read(a1, a2, a3, a4),
        SYS_FILE_CREATE => sys_file_create(a1, a2),
        SYS_FILE_DELETE => sys_file_delete(a1, a2),
        SYS_DIR_CREATE => sys_dir_create(a1, a2),
        SYS_DIR_DELETE => sys_dir_delete(a1, a2),
        _ => EINVAL,  // Unknown syscall
    }
}
```

**Safety**:
- Validates pointers from user space
- Checks buffer lengths
- Prevents access outside user memory window

## Available Syscalls

### SYS_WRITE (1)

**Purpose**: Write data to a file descriptor (stdout/stderr).

**Signature**:
```rust
fn sys_write(fd: usize, buf_ptr: usize, len: usize) -> isize
```

**Parameters**:
- `fd`: File descriptor (1 = stdout, 2 = stderr)
- `buf_ptr`: Pointer to data buffer in user memory
- `len`: Number of bytes to write

**Returns**:
- Success: Number of bytes written
- Error: `EINVAL` if fd is invalid or pointer is bad

**Implementation**: `src/syscall.rs:63-88`

**Process**:
1. Validate file descriptor (only 1 and 2 supported)
2. Validate buffer pointer is in user memory window
3. Convert pointer to slice
4. Write to UART using kernel's `print!` macro
5. Return number of bytes written

**Example** (user space):
```rust
fn write(fd: usize, buf: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 1,              // SYS_WRITE
            in("a1") fd,
            in("a2") buf.as_ptr(),
            in("a3") buf.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage
write(1, b"Hello, world!\n");
```

**Limitations**:
- Only stdout (1) and stderr (2) supported
- No actual distinction between stdout/stderr (both write to UART)
- No file I/O via file descriptors (use SYS_FILE_WRITE instead)

---

### SYS_EXIT (2)

**Purpose**: Terminate the current user process and return to kernel.

**Signature**:
```rust
fn sys_exit(code: usize) -> !
```

**Parameters**:
- `code`: Exit status code (0 = success, non-zero = error)

**Returns**: Never (function does not return)

**Implementation**: `src/syscall.rs:90-99`

**Process**:
1. Set `USER_EXITED` flag to true
2. Store exit code in `USER_EXIT_CODE`
3. Return 0 (trap handler checks flag and returns to kernel instead of user)

**Global State**:
```rust
static mut USER_EXITED: bool = false;
static mut USER_EXIT_CODE: isize = 0;
```

**Trap Handler** (`src/kernel_entry.S:82-106`):
```asm
# After syscall, check if user exited
# If USER_EXITED is true, don't sret, return to kernel
```

**Example** (user space):
```rust
fn exit(code: isize) -> ! {
    unsafe {
        asm!(
            "ecall",
            in("a0") 2,     // SYS_EXIT
            in("a1") code,
            options(noreturn)
        );
    }
}

// Usage
exit(0);  // Successful exit
exit(1);  // Error exit
```

**Kernel Handling** (`src/main.rs:168-180`):
```rust
let exit_code = kernel_resume_from_user();
if exit_code != 0 {
    println!("Process exited with code {}", exit_code);
}
```

---

### SYS_FILE_WRITE (3)

**Purpose**: Write data to a file in the filesystem.

**Signature**:
```rust
fn sys_file_write(
    path_ptr: usize,
    path_len: usize,
    data_ptr: usize,
    data_len: usize
) -> isize
```

**Parameters**:
- `path_ptr`: Pointer to file path string (in user memory)
- `path_len`: Length of path string
- `data_ptr`: Pointer to data buffer (in user memory)
- `data_len`: Length of data to write

**Returns**:
- Success: Number of bytes written
- Error: Negative errno code

**Implementation**: `src/syscall.rs:101-125`

**Process**:
1. Validate pointers are in user memory window
2. Convert pointers to slices
3. Call `FS.lock().write_file(path, data)`
4. Return bytes written or error code

**Example** (user space):
```rust
fn write_file(path: &[u8], data: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 3,                 // SYS_FILE_WRITE
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") data.as_ptr(),
            in("a4") data.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage
let result = write_file(b"/test/file.txt", b"Hello, world!");
if result < 0 {
    // Error handling
}
```

**Errors**:
- `ENOENT` - Parent directory not found
- `ENOSPC` - No space left on device
- `EINVAL` - Invalid pointer or path

---

### SYS_FILE_READ (4)

**Purpose**: Read data from a file in the filesystem.

**Signature**:
```rust
fn sys_file_read(
    path_ptr: usize,
    path_len: usize,
    buf_ptr: usize,
    buf_len: usize
) -> isize
```

**Parameters**:
- `path_ptr`: Pointer to file path string
- `path_len`: Length of path string
- `buf_ptr`: Pointer to buffer for data
- `buf_len`: Maximum bytes to read

**Returns**:
- Success: Number of bytes read
- Error: Negative errno code

**Implementation**: `src/syscall.rs:127-157`

**Process**:
1. Validate pointers are in user memory window
2. Convert path pointer to slice
3. Call `FS.lock().read_file(path)`
4. Copy data to user buffer (up to buf_len bytes)
5. Return actual bytes copied

**Example** (user space):
```rust
fn read_file(path: &[u8], buf: &mut [u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 4,                 // SYS_FILE_READ
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") buf.as_ptr(),
            in("a4") buf.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage (from cat2)
let mut buf = [0u8; 4096];
let len = read_file(b"/test/hello.txt", &mut buf);
if len < 0 {
    // Error
} else {
    let data = &buf[..len as usize];
    // Use data
}
```

**Limitations**:
- Reads entire file into memory
- File must fit in buffer (max 4096 bytes typical)
- No seeking or partial reads

---

### SYS_FILE_CREATE (5)

**Purpose**: Create an empty file.

**Signature**:
```rust
fn sys_file_create(path_ptr: usize, path_len: usize) -> isize
```

**Parameters**:
- `path_ptr`: Pointer to file path string
- `path_len`: Length of path string

**Returns**:
- Success: 0
- Error: Negative errno code

**Implementation**: `src/syscall.rs:159-176`

**Process**:
1. Validate path pointer
2. Call `FS.lock().write_file(path, &[])`
3. Return 0 on success

**Example** (user space):
```rust
fn create_file(path: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 5,                 // SYS_FILE_CREATE
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage
if create_file(b"/test/newfile.txt") < 0 {
    // Error
}
```

**Note**: Writing to a non-existent file also creates it, so this syscall is optional.

---

### SYS_FILE_DELETE (6)

**Purpose**: Delete a file.

**Signature**:
```rust
fn sys_file_delete(path_ptr: usize, path_len: usize) -> isize
```

**Parameters**:
- `path_ptr`: Pointer to file path string
- `path_len`: Length of path string

**Returns**:
- Success: 0
- Error: Negative errno code

**Implementation**: `src/syscall.rs:178-195`

**Process**:
1. Validate path pointer
2. Call `FS.lock().delete_file(path)`
3. Return 0 on success

**Example** (user space):
```rust
fn delete_file(path: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 6,                 // SYS_FILE_DELETE
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage
if delete_file(b"/test/oldfile.txt") < 0 {
    // Error
}
```

**Errors**:
- `ENOENT` - File not found
- `EINVAL` - Path is a directory

---

### SYS_DIR_CREATE (7)

**Purpose**: Create a directory.

**Signature**:
```rust
fn sys_dir_create(path_ptr: usize, path_len: usize) -> isize
```

**Parameters**:
- `path_ptr`: Pointer to directory path string
- `path_len`: Length of path string

**Returns**:
- Success: 0
- Error: Negative errno code

**Implementation**: `src/syscall.rs:197-214`

**Process**:
1. Validate path pointer
2. Call `FS.lock().create_dir(path)`
3. Return 0 on success

**Example** (user space):
```rust
fn create_dir(path: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 7,                 // SYS_DIR_CREATE
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage
if create_dir(b"/test/newdir") < 0 {
    // Error
}
```

**Errors**:
- `ENOENT` - Parent directory not found
- `EINVAL` - Directory already exists
- `ENOSPC` - No space left

---

### SYS_DIR_DELETE (8)

**Purpose**: Delete an empty directory.

**Signature**:
```rust
fn sys_dir_delete(path_ptr: usize, path_len: usize) -> isize
```

**Parameters**:
- `path_ptr`: Pointer to directory path string
- `path_len`: Length of path string

**Returns**:
- Success: 0
- Error: Negative errno code

**Implementation**: `src/syscall.rs:216-233`

**Process**:
1. Validate path pointer
2. Call `FS.lock().delete_dir(path)`
3. Return 0 on success

**Example** (user space):
```rust
fn delete_dir(path: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 8,                 // SYS_DIR_DELETE
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            lateout("a0") ret,
        );
    }
    ret
}

// Usage
if delete_dir(b"/test/emptydir") < 0 {
    // Error
}
```

**Errors**:
- `ENOENT` - Directory not found
- `ENOTEMPTY` - Directory contains files/subdirs
- `EINVAL` - Path is a file or root directory

---

## Security and Validation

### Pointer Validation

All syscalls that accept pointers from user space validate them:

**Function**: `is_user_pointer_valid` (implicit in syscall handlers)

**Checks**:
1. Pointer is within user memory window (`0x80400000 - 0x80420000`)
2. Pointer + length doesn't overflow
3. Entire buffer is within user memory

**Example**:
```rust
const USER_BASE: usize = 0x80400000;
const USER_SIZE: usize = 128 * 1024;

fn validate_user_buffer(ptr: usize, len: usize) -> bool {
    ptr >= USER_BASE &&
    ptr + len <= USER_BASE + USER_SIZE &&
    ptr + len >= ptr  // No overflow
}
```

**Result**: If validation fails, syscall returns `EINVAL`.

### String Validation

For syscalls that accept paths:

1. Validate pointer and length
2. Convert to slice: `core::slice::from_raw_parts(ptr, len)`
3. Optionally validate UTF-8 (filesystem accepts raw bytes)

**Safety**:
- No null-terminator required (length-based)
- Buffer cannot extend outside user memory
- Kernel never trusts user-provided pointers

### Privilege Separation

**User Mode**:
- Cannot access kernel memory
- Cannot execute privileged instructions
- Cannot access MMIO devices
- Must use syscalls for all I/O

**Kernel Mode**:
- Full access to all memory
- Can access devices
- Can modify page tables (if implemented)
- Validates all user input

## Syscall Numbers

**Defined in** `src/syscall.rs:1-12`:

```rust
pub const SYS_WRITE: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_FILE_WRITE: usize = 3;
pub const SYS_FILE_READ: usize = 4;
pub const SYS_FILE_CREATE: usize = 5;
pub const SYS_FILE_DELETE: usize = 6;
pub const SYS_DIR_CREATE: usize = 7;
pub const SYS_DIR_DELETE: usize = 8;
```

**Convention**: Follows general Unix syscall numbering (but not exact).

## Adding New Syscalls

To add a new syscall:

### 1. Define Syscall Number

```rust
// src/syscall.rs
pub const SYS_MYNEWCALL: usize = 9;
```

### 2. Implement Handler

```rust
// src/syscall.rs
fn sys_mynewcall(arg0: usize, arg1: usize) -> isize {
    // Validate arguments
    // Perform operation
    // Return result or error
    0
}
```

### 3. Add to Dispatcher

```rust
// src/syscall.rs - syscall_handler()
match syscall_num {
    // ... existing cases ...
    SYS_MYNEWCALL => sys_mynewcall(a0, a1),
    _ => EINVAL,
}
```

### 4. User-Space Wrapper

```rust
// user_bin/src/lib.rs
fn mynewcall(arg0: usize, arg1: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a0") 9,        // SYS_MYNEWCALL
            in("a1") arg0,
            in("a2") arg1,
            lateout("a0") ret,
        );
    }
    ret
}
```

### 5. Documentation

Update this file with the new syscall's documentation.

## Comparison with POSIX

### Similarities

- Errno-style error codes
- File descriptor concept (stdout/stderr)
- Path-based file operations
- Similar semantics for read/write/create/delete

### Differences

| POSIX | Crabv6 | Notes |
|-------|--------|-------|
| `open()` | None | Direct read/write by path |
| `read(fd, ...)` | `read_file(path, ...)` | No file descriptors for files |
| `write(fd, ...)` | `write_file(path, ...)` | No file descriptors for files |
| `close()` | None | No open file handles |
| `lseek()` | None | No seeking |
| `stat()` | None | No file metadata |
| `fork()` | None | No process creation |
| `exec()` | None | Kernel loads programs |

**Rationale**: Simplified syscall interface focuses on educational clarity over POSIX compatibility.

## Future Enhancements

Potential syscall additions:

- [ ] `SYS_OPEN` / `SYS_CLOSE` - File descriptor-based I/O
- [ ] `SYS_SEEK` - Seeking within files
- [ ] `SYS_STAT` - File metadata
- [ ] `SYS_FORK` - Process creation
- [ ] `SYS_EXEC` - Execute program
- [ ] `SYS_WAIT` - Wait for child process
- [ ] `SYS_SBRK` - Heap allocation
- [ ] `SYS_MMAP` - Memory mapping
- [ ] `SYS_GETPID` / `SYS_GETPPID` - Process IDs
- [ ] `SYS_PIPE` - IPC pipes
- [ ] `SYS_KILL` - Send signals
- [ ] `SYS_SIGACTION` - Signal handling

---

## Summary

The Crabv6 syscall interface provides:
- Clean separation between user and kernel space
- Standard RISC-V calling convention
- Errno-style error handling
- Essential file and I/O operations
- Safe pointer validation
- Simple implementation suitable for teaching

The interface demonstrates fundamental OS concepts while remaining simple enough to understand completely.
