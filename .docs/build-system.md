# Build System and Running Crabv6

This document describes how to build, run, and debug Crabv6, including the build system architecture and tooling.

## Prerequisites

### Required Tools

1. **Rust Toolchain**:
   - Rust nightly (for unstable features)
   - `cargo` build tool
   - `rustup` for toolchain management

2. **RISC-V Target Support**:
   - `riscv64gc-unknown-none-elf` (kernel target)
   - `riscv64imac-unknown-none-elf` (user program target)

3. **QEMU**:
   - `qemu-system-riscv64` version 5.0 or later
   - VirtIO and MMIO support

4. **Optional Tools**:
   - `rust-objdump` for binary inspection
   - `rust-objcopy` for format conversion
   - `gdb-multiarch` for debugging

### Installation

```bash
# Install Rust nightly
rustup install nightly
rustup default nightly

# Add RISC-V targets
rustup target add riscv64gc-unknown-none-elf
rustup target add riscv64imac-unknown-none-elf

# Install QEMU (Ubuntu/Debian)
sudo apt-get install qemu-system-misc

# Install QEMU (macOS)
brew install qemu

# Install development tools
cargo install cargo-binutils
rustup component add llvm-tools-preview
```

## Build Architecture

### Two-Stage Build Process

Crabv6 uses a two-stage build:

1. **User Programs**: Built first by `build.rs`
2. **Kernel**: Built by cargo, embeds user programs

### Stage 1: User Program Build

**Build Script**: `build.rs`

```rust
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=user_bin/");

    // Build user program
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

**Key Points**:
- Runs before kernel build
- Uses `-Z build-std` to rebuild `core` for bare-metal target
- Builds user programs with `riscv64imac` (no compressed or GC extensions)
- Output: `user_bin/target/riscv64imac-unknown-none-elf/release/cat2`

**Rebuild Triggers**:
- Any change in `user_bin/` directory
- Explicit cargo clean

### Stage 2: Kernel Build

**Cargo Configuration**: `Cargo.toml`

```toml
[package]
name = "crabv6"
version = "0.1.0"
edition = "2021"

[dependencies]
riscv = "0.10"
spin = "0.9"
linked_list_allocator = "0.10"

[profile.release]
opt-level = 2
debug = true
lto = true
codegen-units = 1
```

**Target**: `riscv64gc-unknown-none-elf`
- G: General (includes IMAFD)
- C: Compressed instructions
- Bare-metal (no OS)

**Linker Script**: `memory.x`

```ld
MEMORY {
    RAM : ORIGIN = 0x80000000, LENGTH = 32M
}

SECTIONS {
    .text : {
        *(.text.init)
        *(.text*)
    } > RAM

    .rodata : {
        *(.rodata*)
    } > RAM

    .data : {
        *(.data*)
    } > RAM

    .bss : {
        *(.bss*)
    } > RAM
}
```

**Kernel Start Address**: `0x80200000` (defined in `.cargo/config.toml`)

## Cargo Configuration

**File**: `.cargo/config.toml`

```toml
[build]
target = "riscv64gc-unknown-none-elf"

[target.riscv64gc-unknown-none-elf]
rustflags = [
    "-C", "link-arg=-Tmemory.x",
    "-C", "link-arg=--entry=_start",
]
runner = '''
qemu-system-riscv64
  -m 2G
  -machine virt
  -nographic
  -bios none
  -kernel
  -drive file=./disk.img,if=none,id=fsdisk,format=raw
  -device virtio-blk-device,drive=fsdisk,bus=virtio-mmio-bus.0
  -global virtio-mmio.force-legacy=off
'''

[unstable]
build-std = ["core", "compiler_builtins", "alloc"]
build-std-features = ["compiler-builtins-mem"]
```

**Key Settings**:
- **target**: Default to RISC-V 64-bit bare-metal
- **rustflags**: Linker arguments for custom memory layout
- **runner**: QEMU command for `cargo run`
- **build-std**: Rebuild standard library for bare-metal

## User Program Configuration

**Directory**: `user_bin/`

**Cargo.toml**:
```toml
[package]
name = "cat2"
edition = "2021"

[dependencies]

[profile.release]
opt-level = "z"        # Optimize for size
lto = true             # Link-time optimization
codegen-units = 1      # Single codegen unit
panic = "abort"        # No unwinding
strip = true           # Strip symbols
```

**Cargo Config**: `user_bin/.cargo/config.toml`

```toml
[build]
target = "riscv64imac-unknown-none-elf"

[target.riscv64imac-unknown-none-elf]
rustflags = [
    "-C", "link-arg=-Tmemory.x",
]

[unstable]
build-std = ["core", "compiler_builtins"]
build-std-features = ["compiler-builtins-mem"]
```

**Linker Script**: `user_bin/memory.x`

```ld
MEMORY {
    ROM : ORIGIN = 0x80400000, LENGTH = 64K
    RAM : ORIGIN = 0x80410000, LENGTH = 64K
}

SECTIONS {
    .text : { *(.text.init) *(.text*) } > ROM
    .rodata : { *(.rodata*) } > ROM
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
```

**Target Differences**:
| Feature | Kernel (gc) | User (imac) |
|---------|-------------|-------------|
| Integer | ✓ | ✓ |
| Multiply/Divide | ✓ | ✓ |
| Atomics | ✓ | ✓ |
| Float (F) | ✓ | ✗ |
| Double (D) | ✓ | ✗ |
| Compressed | ✓ | ✗ |

**Rationale**: Smaller user binaries, no FP context save needed.

## Building

### Quick Build and Run

```bash
# Build and run (creates disk.img if needed)
cargo run --release

# Build only (no run)
cargo build --release

# Clean build
cargo clean
cargo run --release
```

### Manual Build Steps

```bash
# 1. Create disk image (first time only)
dd if=/dev/zero of=disk.img bs=1M count=16

# 2. Build user programs
cd user_bin
cargo build --release --target riscv64imac-unknown-none-elf -Z build-std=core,compiler_builtins
cd ..

# 3. Build kernel
cargo build --release --target riscv64gc-unknown-none-elf

# 4. Run in QEMU
qemu-system-riscv64 \
  -m 2G \
  -machine virt \
  -nographic \
  -bios none \
  -kernel target/riscv64gc-unknown-none-elf/release/crabv6 \
  -drive file=./disk.img,if=none,id=fsdisk,format=raw \
  -device virtio-blk-device,drive=fsdisk,bus=virtio-mmio-bus.0 \
  -global virtio-mmio.force-legacy=off
```

## QEMU Configuration

### Command Line Breakdown

```bash
qemu-system-riscv64 \
  -m 2G \                          # 2GB RAM
  -machine virt \                  # RISC-V virt platform
  -nographic \                     # No graphical output
  -bios none \                     # No BIOS (direct kernel boot)
  -kernel <binary> \               # Kernel binary
  -drive file=./disk.img,if=none,id=fsdisk,format=raw \
                                   # Disk image (raw format)
  -device virtio-blk-device,drive=fsdisk,bus=virtio-mmio-bus.0 \
                                   # VirtIO block device
  -global virtio-mmio.force-legacy=off
                                   # Disable legacy VirtIO v1
```

### QEMU Machine Layout

**virt Machine**:
- RISC-V 64-bit processor
- PLIC interrupt controller at `0x0c000000`
- UART at `0x10000000`
- VirtIO MMIO devices starting at `0x10001000`
- RAM at `0x80000000`

**Boot Process**:
1. QEMU loads kernel at `0x80200000`
2. Sets PC to kernel entry point
3. Hart 0 starts executing kernel
4. Other harts (if enabled) start at same address

### Exiting QEMU

**From guest**:
```
/> shutdown
```

**From host**:
- Press `Ctrl-A`, then `x`
- Or kill QEMU process

## Embedding User Programs

**Mechanism**: `include_bytes!` macro

**File**: `src/embedded.rs`

```rust
pub static CAT2_BINARY: &[u8] = include_bytes!(
    "../user_bin/target/riscv64imac-unknown-none-elf/release/cat2"
);
```

**Installation**: `src/main.rs:104-109`

```rust
// On first boot, install embedded programs
if !fs.file_exists("/bin/cat2")? {
    fs.write_file("/bin/cat2", CAT2_BINARY)?;
}
```

**Advantages**:
- User programs available immediately
- No need for separate filesystem creation step
- Programs can be updated via filesystem

**Disadvantages**:
- Increases kernel binary size
- Must rebuild kernel to update user programs

## Inspecting Binaries

### objdump

```bash
# Disassemble kernel
rust-objdump -d target/riscv64gc-unknown-none-elf/release/crabv6

# Show sections
rust-objdump -h target/riscv64gc-unknown-none-elf/release/crabv6

# Show symbols
rust-objdump -t target/riscv64gc-unknown-none-elf/release/crabv6
```

### size

```bash
# Show section sizes
rust-size target/riscv64gc-unknown-none-elf/release/crabv6
```

### readelf

```bash
# Show ELF headers
readelf -h target/riscv64gc-unknown-none-elf/release/crabv6

# Show program headers
readelf -l target/riscv64gc-unknown-none-elf/release/crabv6
```

## Debugging

### GDB Setup

**Start QEMU with GDB Server**:
```bash
qemu-system-riscv64 \
  -s \              # GDB server on localhost:1234
  -S \              # Wait for GDB to connect
  -m 2G \
  -machine virt \
  -nographic \
  -kernel target/riscv64gc-unknown-none-elf/release/crabv6 \
  -drive file=./disk.img,if=none,id=fsdisk,format=raw \
  -device virtio-blk-device,drive=fsdisk,bus=virtio-mmio-bus.0 \
  -global virtio-mmio.force-legacy=off
```

**Connect GDB**:
```bash
gdb-multiarch target/riscv64gc-unknown-none-elf/release/crabv6
(gdb) target remote :1234
(gdb) break main
(gdb) continue
```

### Common GDB Commands

```gdb
# Set breakpoint
break kernel_entry
break syscall_handler

# Step execution
stepi          # Single instruction
step           # Single source line
next           # Next line (over calls)

# Examine registers
info registers
print/x $pc
print/x $sp

# Examine memory
x/10x 0x80400000    # Hex dump
x/10i $pc           # Disassemble

# Backtrace
backtrace
```

### Debugging User Programs

**Set breakpoint on user entry**:
```gdb
break *0x80400000   # User program start
continue
stepi
```

**Switch between user and kernel**:
GDB follows execution through traps automatically.

## Troubleshooting

### Build Errors

**Error**: `error: no default toolchain configured`
```bash
rustup install nightly
rustup default nightly
```

**Error**: `error: couldn't read target/...: No such file or directory`
```bash
# User program not built
cargo clean
cargo build --release
```

**Error**: `undefined reference to 'memcpy'`
```bash
# Add to .cargo/config.toml:
build-std-features = ["compiler-builtins-mem"]
```

### Runtime Errors

**Error**: No output, QEMU hangs
- Check UART initialization
- Check QEMU command line (`-nographic` required)
- Try adding `-serial mon:stdio`

**Error**: `VirtIO device not found`
- Check QEMU version (need >= 5.0)
- Ensure `-global virtio-mmio.force-legacy=off`
- Check device probe in kernel output

**Error**: `Filesystem format failed`
- Check disk.img exists: `ls -lh disk.img`
- Recreate: `dd if=/dev/zero of=disk.img bs=1M count=16`

**Error**: `Process exited with code -2`
- File not found
- Check path is correct (absolute vs relative)
- List directory: `fs ls /bin`

## Build Optimization

### Release vs Debug

**Debug Build** (larger, slower):
```bash
cargo build
# No optimizations, includes debug symbols
```

**Release Build** (smaller, faster):
```bash
cargo build --release
# Full optimizations, LTO enabled
```

**Comparison**:
| Metric | Debug | Release |
|--------|-------|---------|
| Kernel Size | ~2MB | ~500KB |
| User Binary | ~50KB | ~4KB |
| Boot Time | ~1s | ~0.5s |

### Size Optimization

**For minimal size**:

```toml
[profile.release]
opt-level = "z"    # Optimize for size
lto = true         # Link-time optimization
codegen-units = 1  # Single codegen unit
strip = true       # Strip symbols
panic = "abort"    # No unwinding
```

**Further reduction**:
```bash
# Strip additional symbols
strip target/riscv64gc-unknown-none-elf/release/crabv6
```

## Continuous Integration

### Example GitHub Actions

```yaml
name: Build

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2

      - name: Install Rust nightly
        uses: actions-rs/toolchain@v1
        with:
          toolchain: nightly
          override: true
          components: rust-src

      - name: Add RISC-V targets
        run: |
          rustup target add riscv64gc-unknown-none-elf
          rustup target add riscv64imac-unknown-none-elf

      - name: Create disk image
        run: dd if=/dev/zero of=disk.img bs=1M count=16

      - name: Build
        run: cargo build --release

      - name: Upload artifact
        uses: actions/upload-artifact@v2
        with:
          name: crabv6-kernel
          path: target/riscv64gc-unknown-none-elf/release/crabv6
```

## Alternative Build Methods

### Using Nix

```nix
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    rustup
    qemu
  ];

  shellHook = ''
    rustup default nightly
    rustup target add riscv64gc-unknown-none-elf
    rustup target add riscv64imac-unknown-none-elf
  '';
}
```

### Using Docker

```dockerfile
FROM rust:latest

RUN rustup install nightly && \
    rustup default nightly && \
    rustup target add riscv64gc-unknown-none-elf && \
    rustup target add riscv64imac-unknown-none-elf && \
    apt-get update && \
    apt-get install -y qemu-system-misc

WORKDIR /workspace
```

## Performance Profiling

### QEMU Tracing

```bash
# Trace all interrupts
qemu-system-riscv64 \
  -trace 'plic_*' \
  ...

# Trace VirtIO
qemu-system-riscv64 \
  -trace 'virtio_*' \
  ...
```

### Instruction Counting

```bash
# Count executed instructions
qemu-system-riscv64 \
  -icount shift=0 \
  ...
```

## Summary

The Crabv6 build system:
- Uses standard Rust/Cargo tooling
- Two-stage build (user programs, then kernel)
- Embeds user programs in kernel binary
- Runs in QEMU with VirtIO devices
- Supports debugging with GDB
- Optimized for size in release mode

The build process is straightforward and leverages Rust's excellent cross-compilation support for embedded targets.

## Quick Reference

```bash
# First-time setup
dd if=/dev/zero of=disk.img bs=1M count=16
rustup target add riscv64gc-unknown-none-elf
rustup target add riscv64imac-unknown-none-elf

# Build and run
cargo run --release

# Exit QEMU
# From guest: shutdown
# From host: Ctrl-A, then x

# Debug
qemu-system-riscv64 -s -S ... &
gdb-multiarch target/.../crabv6
(gdb) target remote :1234
```
