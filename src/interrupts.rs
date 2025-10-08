use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use riscv::register::{sie, sstatus};

const PLIC_BASE: usize = 0x0c00_0000;
const PLIC_PRIORITY_BASE: usize = PLIC_BASE;
const PLIC_SENABLE: usize = PLIC_BASE + 0x2080; // Supervisor enable for hart 0
const PLIC_STHRESHOLD: usize = PLIC_BASE + 0x201000; // Supervisor threshold hart 0
const PLIC_SCLAIM: usize = PLIC_BASE + 0x201004; // Supervisor claim/complete hart 0

const UART_IRQ: u32 = 10;

static EVENT_READY: AtomicBool = AtomicBool::new(false);

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

pub fn wait_for_event() {
    if crate::uart::has_pending_byte() {
        return;
    }

    EVENT_READY.store(false, Ordering::Release);
    loop {
        unsafe {
            riscv::asm::wfi();
        }
        if EVENT_READY.swap(false, Ordering::AcqRel) || crate::uart::has_pending_byte() {
            break;
        }
    }
}

pub fn signal_event() {
    EVENT_READY.store(true, Ordering::Release);
}

#[riscv_rt::core_interrupt(riscv::interrupt::Interrupt::SupervisorExternal)]
fn supervisor_external() {
    let claim = unsafe { read32(PLIC_SCLAIM) };
    if claim == UART_IRQ {
        crate::uart::handle_interrupt();
    }
    unsafe {
        write32(PLIC_SCLAIM, claim);
    }
    signal_event();
}

unsafe fn read32(addr: usize) -> u32 {
    unsafe { ptr::read_volatile(addr as *const u32) }
}

unsafe fn write32(addr: usize, value: u32) {
    unsafe { ptr::write_volatile(addr as *mut u32, value) };
}
