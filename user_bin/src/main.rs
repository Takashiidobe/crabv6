#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_WRITE: usize = 1;
const SYS_EXIT: usize = 2;
const SYS_FILE_READ: usize = 4;

const PATH: &str = "cat2.txt";

#[no_mangle]
#[link_section = ".text.start"]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 4096];
    let len = read_file(PATH, &mut buf);
    if len <= 0 {
        write(1, b"cat2: no data\n");
        exit(1);
    }
    let slice = &buf[..len as usize];
    write(1, slice);
    write(1, slice);
    exit(0)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(2)
}

fn write(fd: usize, buf: &[u8]) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_WRITE,
            in("a1") fd,
            in("a2") buf.as_ptr(),
            in("a3") buf.len(),
            lateout("a0") ret,
        );
    }
    ret
}

fn exit(code: isize) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_EXIT,
            in("a1") code as usize,
            options(noreturn)
        );
    }
}

fn read_file(path: &str, buf: &mut [u8]) -> isize {
    let mut ret: isize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") SYS_FILE_READ,
            in("a1") path.as_ptr(),
            in("a2") path.len(),
            in("a3") buf.as_mut_ptr(),
            in("a4") buf.len(),
            lateout("a0") ret,
        );
    }
    ret
}
