# Interrupt Subsystem Overview

`src/interrupts.rs` wires the QEMU `virt` platform's UART into the PLIC
so the kernel can sleep with `wfi` until a character arrives. The module
also exposes a polling helper that higher-level code (the shell) uses to
avoid busy-waiting.

## PLIC configuration

Initialization programs the PLIC for supervisor-context hart 0:

```rust
const PLIC_BASE: usize = 0x0c00_0000;
const UART_IRQ: u32 = 10;

pub fn init() {
    unsafe {
        write32(PLIC_PRIORITY_BASE + (UART_IRQ as usize) * 4, 1);
        let enabled = read32(PLIC_SENABLE);
        write32(PLIC_SENABLE, enabled | (1 << UART_IRQ));
        write32(PLIC_STHRESHOLD, 0);

        sie::set_sext();
        sstatus::set_sie();
    }
}
```

- the UART source is given priority `1`,
- the supervisor enable bitmap gains the UART bit, and
- the supervisor threshold is lowered to `0` so all interrupts are
  forwarded.

Finally, `sie.SE` and `sstatus.SIE` are asserted so S-mode can actually
receive external interrupts.

## UART interrupt handler

The handler uses `riscv-rt`'s `core_interrupt` attribute to register a
supervisor-external service routine. Each entry performs the standard
PLIC claim/complete handshake and forwards the payload to the UART
module:

```rust
#[riscv_rt::core_interrupt(riscv::interrupt::Interrupt::SupervisorExternal)]
fn supervisor_external() {
    let claim = unsafe { read32(PLIC_SCLAIM) };
    if claim == UART_IRQ {
        crate::uart::handle_interrupt();
    }
    unsafe { write32(PLIC_SCLAIM, claim); }
    signal_event();
}
```

`uart::handle_interrupt` drains every available byte from the 16550
receiver FIFO into an in-memory queue, then `signal_event()` flips a
shared `AtomicBool` letting sleepers know something changed.

## Sleeping and wakeup

Callers use `wait_for_event()` to pause until either the UART queue holds
bytes or a fresh interrupt arrives:

```rust
pub fn wait_for_event() {
    if crate::uart::has_pending_byte() {
        return;
    }

    EVENT_READY.store(false, Ordering::Release);
    loop {
        unsafe { riscv::asm::wfi(); }
        if EVENT_READY.swap(false, Ordering::AcqRel)
            || crate::uart::has_pending_byte()
        {
            break;
        }
    }
}
```

The shell calls this helper whenever `uart::read_byte_nonblocking()`
returns `None`, so hart 0 now blocks in `wfi` until the PLIC signals that
the UART has data. Secondary harts are parked in an infinite `wfi`
inside `idle_loop()` and never touch the PLIC context.

## Limitations and future work

- Only hart 0 is wired to the PLIC. Additional harts would need their
  own enable/threshold programming if they were ever expected to service
  interrupts.
- The receive queue is protected by a `spin::Mutex`. A more advanced
  scheduler would replace this with a proper wait queue or channel.
- Transmit interrupts are still disabled; the driver busy-waits on the
  THR-empty bit to send characters. That is sufficient for shell output
  but could be improved.
- There is no external interrupt filtering; unknown interrupt IDs are
  simply acknowledged. As more devices are added this handler should
  dispatch to per-device routines.

Even with these constraints, the PLIC/UART plumbing removes the busy
loop that previously pegged a hart at 100 % and establishes a foundation
for richer interrupt-driven device support.
