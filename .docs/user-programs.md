# User Programs

This document describes how user-space programs are implemented in Crabv6, using `cat2` as the reference example.

## Overview

User programs in Crabv6:
- Run in RISC-V U-mode (user mode) with reduced privileges
- Are compiled as separate `no_std` Rust binaries
- Use a custom `_start` entry point (no standard library)
- Interact with the kernel via system calls
- Are loaded from the filesystem as ELF64 binaries

## cat2 - Reference Implementation

**Location**: `user_bin/src/main.rs`

**Purpose**: A simple file reader that displays file contents twice (demonstrating basic user-mode execution and syscalls).

### Architecture

#### Program Configuration

**Cargo.toml** (`user_bin/Cargo.toml`):
```toml
[package]
name = "cat2"
edition = "2021"

[dependencies]

[profile.release]
opt-level = "z"       # Optimize for size
lto = true            # Link-time optimization
codegen-units = 1     # Single codegen unit for smaller binary
panic = "abort"       # No unwinding
strip = true          # Strip symbols
```

**Build Target**: `riscv64imac-unknown-none-elf`
- No G/C extensions (no compressed instructions or atomics in user space)
- Bare-metal target (no OS)

#### Memory Layout

**Linker Script** (`user_bin/memory.x`):
```
MEMORY {
    ROM : ORIGIN = 0x80400000, LENGTH = 64K
    RAM : ORIGIN = 0x80410000, LENGTH = 64K
}

SECTIONS {
    .text : { *(.text*) } > ROM
    .rodata : { *(.rodata*) } > ROM
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
```

**Memory Map**:
```
0x80400000 - 0x80410000  : Text and read-only data (64KB)
0x80410000 - 0x80420000  : Data and BSS (64KB)
Stack: Grows down from 0x80420000
```

### Entry Point

**Implementation**: `user_bin/src/main.rs:1-15`

```rust
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    main(argc, argv);
    exit(0);
}
```

**Details**:
- `#![no_std]` - No standard library
- `#![no_main]` - Custom entry point (not `main()`)
- `extern "C"` - C calling convention for kernel compatibility
- Parameters: `argc` (argument count), `argv` (argument vector)
- Must never return (marked with `!`)

**Entry from Kernel**: The kernel's `enter_user_trampoline` function:
1. Sets up `argc` in `a0` register
2. Sets up `argv` pointer in `a1` register
3. Executes `sret` to jump to `_start` in U-mode

### Argument Parsing

**Implementation**: `user_bin/src/main.rs:17-45`

```rust
fn main(argc: usize, argv: *const *const u8) {
    if argc < 2 {
        write(1, b"Usage: cat2 <file>\n");
        exit(1);
    }

    // Get first argument (argv[1])
    let arg_ptr = unsafe { *argv.add(1) };

    // Parse C string
    let mut len = 0;
    while unsafe { *arg_ptr.add(len) } != 0 {
        len += 1;
    }

    let filename = unsafe {
        core::slice::from_raw_parts(arg_ptr, len)
    };

    // Convert to string
    let filename_str = core::str::from_utf8(filename).unwrap_or("(invalid)");
}
```

**Stack Layout** (set up by kernel):
```
0x80420000 (stack top)
  - argc (8 bytes)
  - argv[0] pointer (8 bytes) -> "/bin/cat2"
  - argv[1] pointer (8 bytes) -> "filename.txt"
  - ...
  - argv[n-1] pointer (8 bytes)
  - NULL (8 bytes)
  - [padding for 16-byte alignment]
  - Actual string: "/bin/cat2\0"
  - Actual string: "filename.txt\0"
  - ...
```

**Safety**:
- Pointer arithmetic is `unsafe` (requires trust in kernel)
- Bounds checking is manual (checking for null terminator)
- UTF-8 validation uses `unwrap_or` for safety

### System Call Interface

User programs make system calls via inline assembly.

#### `write` - Write to File Descriptor

**Implementation**: `user_bin/src/main.rs:108-119`

```rust
fn write(fd: usize, buf: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a7") 1,              // SYS_WRITE = 1
            in("a0") fd,             // File descriptor
            in("a1") buf.as_ptr(),   // Buffer pointer
            in("a2") buf.len(),      // Buffer length
            lateout("a0") ret,       // Return value
        );
    }
    ret
}
```

**Details**:
- `ecall` instruction triggers trap to kernel
- `a7` holds syscall number (1 = SYS_WRITE)
- `a0-a6` hold arguments
- Return value in `a0` after `ecall` returns

**File Descriptors**:
- `1` - stdout (console output)
- `2` - stderr (error output)

#### `exit` - Exit Process

**Implementation**: `user_bin/src/main.rs:121-129`

```rust
fn exit(code: isize) -> ! {
    unsafe {
        asm!(
            "ecall",
            in("a7") 2,    // SYS_EXIT = 2
            in("a0") code, // Exit code
            options(noreturn)
        );
    }
}
```

**Details**:
- `options(noreturn)` tells compiler this never returns
- Kernel returns control to shell after exit
- Exit code displayed by shell

#### `read_file` - Read File from Filesystem

**Implementation**: `user_bin/src/main.rs:97-106`

```rust
fn read_file(path: &[u8], buf: &mut [u8]) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a7") 4,              // SYS_FILE_READ = 4
            in("a0") path.as_ptr(),  // Path pointer
            in("a1") path.len(),     // Path length
            in("a2") buf.as_ptr(),   // Buffer pointer
            in("a3") buf.len(),      // Buffer length
            lateout("a0") ret,       // Return value (bytes read or error)
        );
    }
    ret
}
```

**Return Value**:
- Positive: Number of bytes read
- Negative: Error code (errno-style)

### Main Logic

**Implementation**: `user_bin/src/main.rs:47-82`

```rust
fn main(argc: usize, argv: *const *const u8) {
    // 1. Check arguments
    if argc < 2 {
        write(1, b"Usage: cat2 <file>\n");
        exit(1);
    }

    // 2. Parse filename from argv[1]
    let filename = /* ... */;

    // 3. Read file
    let mut buf = [0u8; 4096];
    let len = read_file(filename, &mut buf);

    // 4. Check for errors
    if len < 0 {
        write(2, b"Error reading file\n");
        exit(1);
    }

    // 5. Output file contents TWICE (hence "cat2")
    let data = &buf[..len as usize];
    write(1, data);
    write(1, data);

    // 6. Exit successfully
    exit(0);
}
```

**Why "cat2"?**:
The program outputs file contents twice to demonstrate:
- User programs can loop and make multiple syscalls
- System call return values are preserved
- User-mode execution continues until explicit exit

### Panic Handler

**Implementation**: `user_bin/src/main.rs:84-95`

```rust
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let msg = if let Some(location) = info.location() {
        write(2, b"PANIC at ");
        // Write location info...
    } else {
        write(2, b"PANIC: unknown location\n");
    };

    exit(1);
}
```

**Details**:
- Required for `no_std` binaries
- Writes panic message to stderr (fd=2)
- Exits with code 1

## Build Process

User programs are built in a two-stage process:

### Stage 1: Build User Binary

**Build Script**: `build.rs:1-30`

```rust
fn main() {
    // Run cargo build for user program
    let status = Command::new("cargo")
        .args(&[
            "build",
            "--release",
            "--manifest-path", "user_bin/Cargo.toml",
            "--target", "riscv64imac-unknown-none-elf",
            "-Z", "build-std=core,compiler_builtins",
        ])
        .status()
        .expect("Failed to build user program");

    if !status.success() {
        panic!("User program build failed");
    }
}
```

**Output**: `user_bin/target/riscv64imac-unknown-none-elf/release/cat2`

### Stage 2: Embed in Kernel

**Embedding**: `src/embedded.rs:1-5`

```rust
pub static CAT2_BINARY: &[u8] = include_bytes!(
    "../user_bin/target/riscv64imac-unknown-none-elf/release/cat2"
);
```

**Installation**: `src/main.rs:104-109`

```rust
// Install cat2 on first boot
fs.write_file("/bin/cat2", CAT2_BINARY)?;
```

**Rationale**:
- User programs available immediately on boot
- Can be overwritten or deleted like any file
- Demonstrates filesystem integration

## Loading and Execution

### ELF Loading

**Process** (`src/process.rs:131-165`):
1. Read ELF binary from filesystem
2. Parse ELF headers and validate
3. Find PT_LOAD segments
4. Calculate relocation offset
5. Copy segments to user memory window
6. Zero-fill BSS sections
7. Set up stack with arguments
8. Jump to entry point

**Example**:
```
/> run /bin/cat2 /test/hello.txt
```

### Context Switch to User Mode

**Trampoline** (`src/process.rs:37-74`, `src/kernel_entry.S:108-130`):

```rust
// Set up context
sscratch = kernel_stack_ptr;
sepc = user_entry;
sstatus.SPP = 0;  // Return to U-mode
a0 = argc;
a1 = argv_ptr;

// Jump to user mode
sret;
```

**State After `sret`**:
- Mode: U-mode (privilege level 0)
- PC: User program's `_start` function
- SP: Top of user stack (0x80420000)
- a0: argc
- a1: argv pointer
- sscratch: Kernel stack pointer (for trap handling)

### Trap Back to Kernel

When user program calls `ecall`:

1. **Trap to kernel_entry** (`src/kernel_entry.S:1-69`)
2. **Save context** to kernel stack
3. **Call syscall_handler** (`src/syscall.rs:14-50`)
4. **Execute syscall** (write, read, exit, etc.)
5. **Return value in a0**
6. **Restore context** and `sret` to user mode

**Exit Syscall**:
- Sets flag to prevent `sret`
- Returns to kernel's `kernel_resume_from_user`
- Shell displays exit code

## Creating New User Programs

To create a new user program:

### 1. Create Program Directory

```bash
mkdir user_bin_new
cd user_bin_new
cargo init --lib
```

### 2. Configure Cargo.toml

```toml
[package]
name = "mynewprog"

[dependencies]

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

### 3. Add .cargo/config.toml

```toml
[build]
target = "riscv64imac-unknown-none-elf"

[unstable]
build-std = ["core", "compiler_builtins"]
```

### 4. Create memory.x

```
MEMORY {
    ROM : ORIGIN = 0x80400000, LENGTH = 64K
    RAM : ORIGIN = 0x80410000, LENGTH = 64K
}

SECTIONS {
    .text : { *(.text*) } > ROM
    .rodata : { *(.rodata*) } > ROM
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
```

### 5. Implement Program

```rust
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    // Your implementation
    exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
}

// Syscall wrappers...
```

### 6. Update build.rs

Add build step for new program:
```rust
Command::new("cargo")
    .args(&["build", "--manifest-path", "user_bin_new/Cargo.toml", ...])
    .status()?;
```

### 7. Embed and Install

Add to `src/embedded.rs`:
```rust
pub static MYNEWPROG_BINARY: &[u8] = include_bytes!("...");
```

Install on boot in `src/main.rs`.

## Limitations

Current limitations of the user program model:

- **Single Process**: Only one user program can run at a time
- **No Dynamic Linking**: All programs are statically linked
- **Fixed Memory**: 128KB memory window (64KB text + 64KB data/stack)
- **No Heap**: No `malloc`/`free` (could implement custom allocator)
- **No Standard Library**: Must implement everything from scratch
- **No Floating Point**: FP context not saved during traps
- **Limited Syscalls**: Only 8 syscalls available
- **No IPC**: No inter-process communication
- **No Signals**: No signal handling

## Future Enhancements

Potential improvements:

- [ ] Implement `sbrk` syscall for dynamic heap allocation
- [ ] Add more syscalls (open, close, read, write to fds)
- [ ] Support for shared libraries
- [ ] Process table and scheduling
- [ ] Virtual memory with MMU
- [ ] Copy-on-write fork()
- [ ] Pipes and IPC
- [ ] Signal handling
- [ ] User-space heap allocator library

---

## Summary

User programs in Crabv6:
- Are true user-mode binaries with privilege separation
- Use standard ELF format and toolchain
- Demonstrate syscall interface and trap handling
- Provide a clean example of kernel/user boundary
- Are simple enough to understand completely

The `cat2` program serves as both a useful utility and a template for creating additional user programs.
