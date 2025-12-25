use alloc::collections::VecDeque;
use core::ptr;
use spin::Mutex;

const UART0_BASE: usize = 0x1000_0000;
const REG_RBR: usize = 0; // Receiver Buffer Register (read)
const REG_THR: usize = 0; // Transmitter Holding Register (write)
const REG_IER: usize = 1; // Interrupt Enable Register
const REG_FCR: usize = 2; // FIFO Control Register
const REG_LCR: usize = 3; // Line Control Register
const REG_MCR: usize = 4; // Modem Control Register
const REG_LSR: usize = 5; // Line Status Register

const LSR_DATA_READY: u8 = 1 << 0;
const LSR_THR_EMPTY: u8 = 1 << 5;

const IER_RECEIVE_AVAILABLE: u8 = 1 << 0;

static RX_QUEUE: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());

fn read_reg(offset: usize) -> u8 {
    unsafe { ptr::read_volatile((UART0_BASE + offset) as *const u8) }
}

fn write_reg(offset: usize, value: u8) {
    unsafe { ptr::write_volatile((UART0_BASE + offset) as *mut u8, value) }
}

pub fn init() {
    // Configure 16550-compatible UART for 8N1, enable FIFO, and RX interrupts.
    write_reg(REG_LCR, 0x80); // Set DLAB to access divisor registers.
    write_reg(REG_THR, 0x00); // Divisor LSB (ignored by QEMU default clock).
    write_reg(REG_IER, 0x00); // Divisor MSB.
    write_reg(REG_LCR, 0x03); // 8 bits, no parity, one stop bit.
    write_reg(REG_FCR, 0x07); // Enable FIFO, clear RX/TX queues.
    write_reg(REG_MCR, 0x0B); // Assert DTR, RTS, OUT2 (enables interrupts).
    write_reg(REG_IER, IER_RECEIVE_AVAILABLE);
}

pub fn write_byte(byte: u8) {
    while read_reg(REG_LSR) & LSR_THR_EMPTY == 0 {}
    write_reg(REG_THR, byte);
}

pub fn write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        if byte == b'\n' {
            write_byte(b'\r');
        }
        write_byte(byte);
    }
}

pub fn write_str(s: &str) {
    write_bytes(s.as_bytes());
}

pub fn read_byte_nonblocking() -> Option<u8> {
    RX_QUEUE.lock().pop_front()
}

/// Blocking read that also polls the hardware in case interrupts are not delivered.
pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte_nonblocking() {
            return b;
        }
        // Fallback to polling the UART data-ready bit.
        if read_reg(REG_LSR) & LSR_DATA_READY != 0 {
            return read_reg(REG_RBR);
        }
        core::hint::spin_loop();
    }
}

pub fn has_pending_byte() -> bool {
    !RX_QUEUE.lock().is_empty()
}

pub fn handle_interrupt() {
    let mut queue = RX_QUEUE.lock();
    while read_reg(REG_LSR) & LSR_DATA_READY != 0 {
        let byte = read_reg(REG_RBR);
        queue.push_back(byte);
    }
    drop(queue);
    crate::interrupts::signal_event();
}
