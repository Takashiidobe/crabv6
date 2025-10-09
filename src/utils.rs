
pub fn print(t: &str) {
    crate::uart::write_str(t);
}

struct Writer {}

pub fn print_args(t: core::fmt::Arguments) {
    use core::fmt::Write;
    let mut writer = Writer {};
    writer.write_fmt(t).unwrap();
}

impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        print(s);
        Ok(())
    }
}

pub fn shutdown() -> ! {
    let _ = sbi::system_reset::system_reset(
        sbi::system_reset::ResetType::Shutdown,
        sbi::system_reset::ResetReason::NoReason,
    );
    unreachable!("System reset failed");
}

#[macro_export]
macro_rules! print {
    ($fmt:literal$(, $($arg: tt)+)?) => {
        $crate::utils::print_args(format_args!($fmt $(,$($arg)+)?))
    }
}

#[macro_export]
macro_rules! println {
    ($fmt:literal$(, $($arg: tt)+)?) => {{
        $crate::print!($fmt $(,$($arg)+)?);
        $crate::utils::print("\n");
    }};
    () => {
        $crate::utils::print("\n");
    }
}
