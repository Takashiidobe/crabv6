use core::sync::atomic::{AtomicBool, Ordering};

use riscv::register::{self, sie, sstatus, time};

static TIMER_FIRED: AtomicBool = AtomicBool::new(false);
const TIMER_INTERVAL_TICKS: u64 = 100_000; // ~100us at 1 GHz, adjust as needed

pub fn init() {
    unsafe {
        sie::set_stimer();
        sstatus::set_sie();
    }
}

pub fn wait_for_event() {
    arm_timer();
    loop {
        unsafe {
            riscv::asm::wfi();
        }
        if TIMER_FIRED.swap(false, Ordering::AcqRel) {
            break;
        }
    }
}

fn arm_timer() {
    TIMER_FIRED.store(false, Ordering::Release);
    let now = time::read64();
    let deadline = now.wrapping_add(TIMER_INTERVAL_TICKS);
    sbi::timer::set_timer(deadline);
}

#[riscv_rt::core_interrupt(riscv::interrupt::Interrupt::SupervisorTimer)]
fn supervisor_timer() {
    sbi::timer::set_timer(u64::MAX);
    TIMER_FIRED.store(true, Ordering::Release);
}
