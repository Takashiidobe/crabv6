# Kernel Architecture and Design Decisions

This document describes the architecture of the Crabv6 kernel and the key design decisions made during development.

## Overview

Crabv6 is a monolithic kernel running in RISC-V supervisor mode (S-mode) with support for user-mode (U-mode) programs. The design emphasizes simplicity and educational clarity over performance optimization.

## Memory Architecture

### Memory Layout

The kernel uses a simple physical memory layout without virtual memory or paging:

```
Physical Memory Map:
0x80000000 - 0x80200000  : Kernel code and data (2MB reserved)
0x80200000 - 0x80220000  : Kernel heap (128KB)
0x80400000 - 0x80420000  : User memory window (128KB)
  0x80400000 - 0x80410000  : User text/data segments (64KB)
  0x80410000 - 0x80420000  : User stack (64KB, grows down)

MMIO Regions:
0x10000000 : UART0 (16550)
0x10001000 : VirtIO block device
0x0c000000 : PLIC interrupt controller
```

### Design Decision: No Virtual Memory

**Choice**: Run entirely with physical addressing, no MMU/paging.

**Rationale**:
- Simplifies the implementation for educational purposes
- Avoids complexity of page table management
- Still demonstrates privilege separation (S-mode vs U-mode)
- User programs run in fixed memory window with hardware privilege enforcement

**Trade-offs**:
- ✅ Simpler to understand and debug
- ✅ No TLB management overhead
- ❌ No memory protection between processes (single process model only)
- ❌ Limited to one user program at a time
- ❌ No demand paging or memory overcommit

### Kernel Heap

Implementation: `src/heap.rs:1-30`

**Choice**: Static 128KB heap with linked-list allocator.

**Details**:
```rust
static mut KERNEL_HEAP: [u8; 128 * 1024] = [0; 128 * 1024];
```

Uses the `linked_list_allocator` crate for simple heap management.

**Rationale**:
- Fixed-size heap simplifies allocation
- 128KB is sufficient for kernel data structures and buffers
- Linked-list allocator is simple and doesn't require page alignment

**Trade-offs**:
- ✅ Simple implementation
- ✅ No fragmentation issues with careful usage
- ❌ No dynamic heap growth
- ❌ Can fragment with many allocations

### User Memory Management

Implementation: `src/process.rs:131-165`

**Choice**: Fixed 128KB window at `0x80400000`.

**Details**:
- User programs are loaded via ELF loader
- Position-independent loading: ELF segments are relocated to base address
- Stack is set up at top of window (`0x80420000`)
- BSS sections are zero-filled

**Rationale**:
- Predictable memory layout
- Simplifies ELF loading (single base address)
- Stack grows down, heap could grow up (not currently implemented)

## Process Model

### Design Decision: Single-Process Execution

Implementation: `src/process.rs`

**Choice**: Load-and-run model, no scheduler or multitasking.

**How it Works**:
1. Shell receives `run <path> [args...]` command
2. Kernel loads ELF binary from filesystem into user window
3. Kernel sets up stack with argc/argv
4. Kernel jumps to user program entry point via `enter_user_trampoline`
5. User program runs until it calls `sys_exit`
6. Control returns to kernel shell

**Rationale**:
- Simplest process model for teaching
- Focuses on privilege transitions and system calls
- Avoids complexity of context switching between processes
- Still demonstrates user/kernel boundary

**Trade-offs**:
- ✅ Simple to understand and implement
- ✅ No scheduler complexity
- ❌ No concurrency or multitasking
- ❌ No background processes
- ❌ System is idle when user program runs

### ELF Loading

Implementation: `src/elf.rs`, `src/process.rs:131-165`

**Choice**: Full ELF64 parser with position-independent loading.

**Details**:
- Parses ELF headers and program headers
- Supports multiple PT_LOAD segments
- Calculates minimum virtual address and relocates all segments
- Zero-fills BSS sections
- Validates ELF magic and architecture

**Rationale**:
- Standard binary format used by toolchains
- Position-independent loading allows flexible memory layout
- Proper ELF support enables standard compilation toolchain

**Notable Implementation**:
```rust
// Calculate base address for relocation
let min_vaddr = load_segments.iter()
    .map(|seg| seg.p_vaddr)
    .min()
    .unwrap_or(0);

// Load each segment with relocation
for segment in load_segments {
    let offset = (segment.p_vaddr - min_vaddr) as usize;
    let dest = &mut user_mem[offset..offset + segment.p_filesz as usize];
    dest.copy_from_slice(&elf_data[...]);
}
```

### Argument Passing

Implementation: `src/process.rs:76-129`

**Choice**: Standard Unix argc/argv on stack.

**Stack Layout**:
```
[Top of stack: 0x80420000]
  argc          (8 bytes)
  argv[0]       (8 bytes, pointer to program name)
  argv[1]       (8 bytes, pointer to first arg)
  ...
  argv[n-1]     (8 bytes)
  NULL          (8 bytes)
  <padding for alignment>
  [arg strings]  (null-terminated C strings)
[Stack grows down]
```

**Rationale**:
- Standard convention for Unix-like systems
- Allows user programs to parse arguments naturally
- Demonstrates kernel/user data marshalling

**Implementation Details**:
- Strings are copied to user stack
- 16-byte alignment is maintained
- Supports up to 16 arguments (arbitrary limit)
- Proper null termination

## Trap Handling

### Context Switching

Implementation: `src/kernel_entry.S:1-106`

**Choice**: Assembly trampoline with `sscratch` register swapping.

**Mechanism**:
```assembly
kernel_entry:
    # Swap sscratch and sp
    csrrw sp, sscratch, sp
    bnez sp, from_user_mode

from_kernel_mode:
    # sscratch was zero, trap from kernel
    csrr sp, sscratch

from_user_mode:
    # Save context to kernel stack
    sd x1, 8(sp)
    sd x2, 16(sp)
    ...
```

**Rationale**:
- `sscratch` holds kernel stack pointer when in user mode
- `sscratch` is zero when in kernel mode
- Allows single trap handler to distinguish user vs kernel traps
- Minimal context save (16 general-purpose registers only)

**Trade-offs**:
- ✅ Efficient single-handler design
- ✅ Standard RISC-V trap handling pattern
- ❌ No floating-point context saved (user programs can't use FP)

### System Call Dispatch

Implementation: `src/kernel_entry.S:70-81`, `src/syscall.rs:14-50`

**Flow**:
1. User program executes `ecall` instruction
2. Trap to `kernel_entry` (from user mode)
3. Save context to kernel stack
4. Call `syscall_handler(a0, a1, a2, a3, a4, a5, a6, a7)`
   - `a7` contains syscall number
   - `a0-a6` contain arguments
5. Return value in `a0`
6. Restore context and `sret` to user mode

**Rationale**:
- Standard RISC-V calling convention
- Up to 6 arguments supported
- Return value in `a0` (or error code if negative)

### Error Handling

Implementation: `src/syscall.rs:52-61`

**Choice**: Negative return values indicate errors (errno-style).

**Error Codes**:
```rust
pub const ENOENT: isize = -2;   // No such file or directory
pub const EINVAL: isize = -22;  // Invalid argument
pub const ENOSPC: isize = -28;  // No space left on device
pub const ENOTEMPTY: isize = -39; // Directory not empty
```

**Rationale**:
- Compatible with Unix errno convention
- Simple error propagation (no complex Result types at syscall boundary)
- User programs can check for negative return values

## Interrupt Handling

### Design Decision: Event-Driven Console

Implementation: `src/main.rs:31-67`, `src/interrupts.rs`

**Choice**: Interrupt-driven UART with `wfi` sleep.

**Evolution**:
- **Initially**: Busy-wait polling on UART
- **Current**: PLIC interrupts wake kernel from `wfi`

**Mechanism**:
```rust
static UART_EVENT: AtomicBool = AtomicBool::new(false);

fn main_loop() {
    loop {
        if UART_EVENT.load(Ordering::Acquire) {
            UART_EVENT.store(false, Ordering::Release);
            // Process input
        }
        wfi(); // Sleep until interrupt
    }
}

fn handle_uart_interrupt() {
    UART_EVENT.store(true, Ordering::Release);
}
```

**Rationale**:
- Eliminates 100% CPU usage when idle
- Demonstrates interrupt-driven I/O
- Simple event signaling with atomics

**Trade-offs**:
- ✅ Power-efficient (CPU sleeps)
- ✅ Responsive to input
- ❌ Slightly more complex than polling
- ❌ UART RX only (TX still polls)

### PLIC Configuration

Implementation: `src/interrupts.rs:44-60`

**Details**:
- UART interrupt source: 10
- Priority: 1 (lowest non-zero)
- Routed to supervisor mode (hart 0)
- Edge-triggered

**Rationale**:
- PLIC is the standard RISC-V interrupt controller
- Simple priority scheme (only one interrupt source)
- Demonstrates proper interrupt controller setup

## Device Drivers

### UART Driver

Implementation: `src/uart.rs`

**Choice**: Interrupt-driven receive, polling transmit.

**Receive Path**:
- UART interrupt on RX ready
- ISR reads character and queues in kernel buffer
- Buffer: `VecDeque<u8>` with 256-byte capacity

**Transmit Path**:
- Busy-wait on THR empty bit
- Direct character output

**Rationale**:
- RX interrupts prevent losing characters
- TX polling is simple (small output buffers)
- Asymmetric design matches usage pattern (more RX than TX typically)

### VirtIO Block Driver

Implementation: `src/virtio.rs`

**Choice**: Synchronous polling driver (no interrupts).

**Details**:
- VirtIO-MMIO v2 protocol
- Single virtqueue with 8 descriptors
- 512-byte sector I/O
- Rejects legacy (v1) devices

**Mechanism**:
```rust
pub fn read_block(&mut self, block_num: u32, buf: &mut [u8; 512]) {
    // 1. Setup descriptor chain
    // 2. Write to avail ring
    // 3. Notify device
    // 4. Poll for completion
    // 5. Read from used ring
    // 6. Copy data
}
```

**Rationale**:
- Synchronous I/O simplifies filesystem code
- VirtIO is standard for QEMU
- Polling is acceptable for teaching (low I/O volume)
- Demonstrates virtqueue protocol

**Trade-offs**:
- ✅ Simple implementation
- ✅ No interrupt handler complexity
- ❌ Blocks kernel during I/O
- ❌ Cannot do concurrent operations

### Design Decision: VirtIO v2 Only

**Choice**: Explicitly reject legacy VirtIO v1 devices.

**Code**: `src/virtio.rs:47-51`
```rust
if device.version != 2 {
    return Err(VirtioError::UnsupportedVersion);
}
```

**Rationale**:
- VirtIO v2 is current standard
- Simpler protocol (no legacy quirks)
- QEMU supports v2 by default with `force-legacy=off`

## Multi-Hart Support

Implementation: `src/main.rs:90-102`

**Choice**: Hart 0 runs kernel, others idle with `wfi`.

**Code**:
```rust
let hart_id: usize;
unsafe { asm!("mv {}, tp", out(reg) hart_id) }

if hart_id == 0 {
    // Run kernel
    kernel_main();
} else {
    // Other harts sleep forever
    loop { wfi(); }
}
```

**Rationale**:
- Simple single-core design for teaching
- Avoids SMP complexity
- Other harts don't interfere

**Future Extension**:
- Could wake hart 1+ for specific tasks
- Could implement simple SMP scheduler

## Summary of Key Design Decisions

| Aspect | Choice | Rationale |
|--------|--------|-----------|
| Memory Management | No virtual memory, physical addressing | Simplicity for teaching |
| Process Model | Single-process, no scheduler | Focus on privilege transitions |
| Heap Allocation | Static 128KB linked-list | Simple, sufficient for kernel needs |
| User Memory | Fixed 128KB window | Predictable, simple ELF loading |
| Interrupts | Event-driven UART, polling VirtIO | Balance of simplicity and efficiency |
| Trap Handling | sscratch swapping | Standard RISC-V pattern |
| Syscalls | errno-style return codes | Unix-compatible, simple |
| ELF Loading | Position-independent relocation | Flexible, standard toolchain support |
| Filesystem | Custom TinyFS | Educational, demonstrates concepts |

All decisions prioritize **educational clarity** over performance, making the codebase ideal for learning operating system fundamentals.
