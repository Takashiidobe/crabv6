# Interrupt Subsystem Overview

The `src/interrupts.rs` module provides the minimal plumbing required to
let harts sleep with `wfi` and wake back up when there is work to do. It
currently relies on the RISC-V supervisor timer rather than wiring a
real UART interrupt, but the structure is ready for future extensions.

## Timer-driven wakeups

At the heart of the module is a single atomic flag and a helper that
arms the SBI timer:

```rust
static TIMER_FIRED: AtomicBool = AtomicBool::new(false);
const TIMER_INTERVAL_TICKS: u64 = 100_000; // ~10 ms at 10 MHz, adjust as needed

fn arm_timer() {
    TIMER_FIRED.store(false, Ordering::Release);
    let now = time::read64();
    let deadline = now.wrapping_add(TIMER_INTERVAL_TICKS);
    sbi::timer::set_timer(deadline);
}
```

`arm_timer` schedules the next wakeup by calling `sbi::timer::set_timer`
with a deadline derived from the current `time` CSR. When the wakeup
occurs, the handler stores `true` in `TIMER_FIRED` and disables further
ticks by resetting the deadline to `u64::MAX`.

## Interrupt handler

Using `riscv-rt`'s `core_interrupt` attribute, the module defines a
supervisor timer handler that simply sets the flag:

```rust
#[riscv_rt::core_interrupt(riscv::interrupt::Interrupt::SupervisorTimer)]
fn supervisor_timer() {
    sbi::timer::set_timer(u64::MAX);
    TIMER_FIRED.store(true, Ordering::Release);
}
```

Because `Interrupt::SupervisorTimer` maps to the standard S-mode timer
interrupt, this handler runs whenever the SBI fires the guard timer.

## Initialization

Before `wfi` is useful, supervisor interrupt delivery needs to be
enabled. `init()` takes care of that by setting the `STIE` bit in `sie`
and the global SIE bit in `sstatus`:

```rust
pub fn init() {
    unsafe {
        sie::set_stimer();
        sstatus::set_sie();
    }
}
```

`main` calls `interrupts::init()` once, right after setting up the heap.

## Blocking wait primitive

The shell and any other spinner call `wait_for_event()` instead of tight
polling loops:

```rust
pub fn wait_for_event() {
    arm_timer();
    loop {
        unsafe { riscv::asm::wfi(); }
        if TIMER_FIRED.swap(false, Ordering::AcqRel) {
            break;
        }
    }
}
```

This function sets the timer deadline, executes `wfi`, and only returns
when the handler has observed the interrupt. The `swap` call drains the
flag so subsequent calls force a new timer arm.

## Current limitations

- The wakeup cadence is timer-driven (~10 ms) and does not yet respond
  directly to UART data. Moving to a UART/PLIC interrupt would eliminate
  the delay between keypresses and wakeups.
- There is no per-hart timer state; every hart reuses the shared flag, so
  simultaneous timer waits would be racy. At the moment only hart 0 uses
  `wait_for_event`, and other harts spend their time in an infinite WFI
  loop inside `idle_loop()`.
- Because the timer deadline is global, any additional users should
  coordinate via a scheduler before arming their own wakeups.

Despite those constraints, the module provides enough infrastructure to
idle CPUs responsibly while keeping the system responsive under light
load. Future extensions can plug in real interrupt sources by adding new
handlers with `riscv_rt::core_interrupt` or `external_interrupt`.
