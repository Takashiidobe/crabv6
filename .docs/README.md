# Crabv6 - xv6-style Operating System in Rust

Crabv6 is an educational operating system written in Rust for the RISC-V 64-bit architecture. Inspired by MIT's xv6, it demonstrates fundamental operating system concepts with a clean, minimal implementation suitable for learning OS internals.

## Overview

This is a teaching OS that runs in RISC-V supervisor mode (S-mode) and supports user-mode (U-mode) programs. It includes:

- Interactive kernel shell with builtin commands
- VirtIO-based block device driver
- Simple custom filesystem (TinyFS)
- ELF binary loader
- System call interface
- Interrupt-driven UART I/O
- User-space program support

**Codebase Size**: ~2,500 lines of Rust + ~100 lines of assembly

## Key Features

### Kernel Features
- **Boot and Initialization**: Multi-hart aware (hart 0 runs kernel, others idle)
- **Memory Management**: 128KB kernel heap, fixed 128KB user memory window
- **Interrupt Handling**: PLIC-based interrupt controller with UART interrupts
- **Device Drivers**: 16550 UART and VirtIO block device
- **Filesystem**: TinyFS with hierarchical directories on 16MB disk image
- **Process Loading**: ELF loader with position-independent relocation

### Shell Commands
- **Builtin Commands**: `echo`, `help`, `clear`, `shutdown`
- **Filesystem Commands**: `fs ls`, `fs cat`, `fs write`, `fs mkdir`, `fs rm`, `fs cd`, `fs format`
- **Program Execution**: `run <path> [args...]` to execute user binaries

### User Programs
- **cat2**: A simple file reader that displays file contents twice (demonstrating user-mode execution)
- Full argc/argv support for command-line arguments
- System call interface for file I/O and process control

## Quick Start

### Prerequisites
- Rust nightly toolchain
- QEMU RISC-V system emulator
- `riscv64-unknown-elf` target support

### Building and Running

```bash
# Create filesystem image (first time only)
dd if=/dev/zero of=disk.img bs=1M count=16

# Build and run
cargo run --release
```

### Example Session

```
/> help
Available commands:
  echo <text>           - Print text
  fs ls [path]          - List directory
  fs cat <path>         - Display file
  fs write <path> <text> - Write to file
  run <path> [args...]  - Execute program

/> fs format
Filesystem formatted

/> fs mkdir /test

/> fs write /test/hello.txt "Hello, world!"

/> fs cat /test/hello.txt
Hello, world!

/> run /bin/cat2 /test/hello.txt
Hello, world!
Hello, world!
```

## Architecture

### Memory Layout

```
0x80000000 - 0x80200000  : Kernel code/data (first 2MB, not all used)
0x80200000 - 0x80400000  : Kernel heap (128KB at start of this region)
0x80400000 - 0x80420000  : User memory window (128KB)
  0x80400000 - 0x80410000  : User program code/data (64KB)
  0x80410000 - 0x80420000  : User stack (64KB, grows down)
```

### MMIO Regions

```
0x10000000 : UART (16550)
0x10001000 : VirtIO block device
0x0c000000 : PLIC (interrupt controller)
```

## Documentation

Detailed documentation is available in the following files:

- [Architecture](architecture.md) - Kernel architecture and design decisions
- [Builtins](builtins.md) - Builtin shell commands
- [User Programs](user-programs.md) - User-space program implementation
- [Filesystem](filesystem.md) - TinyFS design and implementation
- [System Calls](syscalls.md) - System call interface
- [Build System](build-system.md) - Build process and tooling

## Project Structure

```
crabv6/
├── src/                  - Kernel source code
│   ├── main.rs           - Entry point, shell, command processing
│   ├── process.rs        - Process loading and user-mode execution
│   ├── syscall.rs        - System call dispatcher and handlers
│   ├── fs.rs             - TinyFS filesystem implementation
│   ├── virtio.rs         - VirtIO block device driver
│   ├── elf.rs            - ELF binary parser
│   ├── heap.rs           - Kernel heap allocator
│   ├── interrupts.rs     - PLIC interrupt controller
│   ├── uart.rs           - 16550 UART driver
│   ├── kernel_entry.S    - Assembly trap handlers
│   └── ...
├── user_bin/             - User-space programs
│   └── src/main.rs       - cat2 utility
├── .docs/                - Documentation
├── build.rs              - Build script
├── memory.x              - Linker script
└── disk.img              - Filesystem image
```

## Design Philosophy

Crabv6 prioritizes **clarity and correctness over performance**, making it ideal for educational purposes:

- Simple, direct implementations of OS concepts
- Minimal abstraction layers
- Well-commented code explaining design decisions
- No unnecessary optimizations that obscure understanding
- Proper error handling throughout

## Learning Goals

This OS demonstrates:

1. **Privilege Modes**: S-mode kernel, U-mode user programs
2. **Trap Handling**: Context switching, system call dispatch
3. **Memory Management**: Simple heap allocation, user/kernel separation
4. **I/O**: Interrupt-driven UART, polling VirtIO
5. **Filesystems**: Block-based storage, directory hierarchies
6. **Process Loading**: ELF parsing, relocation, execution
7. **Safe Systems Programming**: Using Rust for OS development

## Limitations

As a teaching OS, Crabv6 has intentional limitations:

- No virtual memory or MMU usage (physical addressing only)
- Single-process execution model (no scheduler)
- Simple bump allocator for filesystem (no free list)
- Limited root directory size (11 entries)
- No multi-threading or SMP support
- Synchronous I/O only

## License

See LICENSE file for details.

## References

- [xv6: A simple, Unix-like teaching operating system](https://pdos.csail.mit.edu/6.828/2020/xv6.html)
- [RISC-V Privileged Architecture Specification](https://riscv.org/technical/specifications/)
- [The Rust Programming Language](https://www.rust-lang.org/)
