use core::cell::UnsafeCell;
use core::fmt;
use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
pub struct ReadOnly<T: Copy> {
    value: UnsafeCell<T>
}

impl<T: Copy> ReadOnly<T> {
    #[inline(always)]
    pub fn read(&self) -> T {
        unsafe { read_volatile(self.value.get())}
    }
}

#[repr(transparent)]
pub struct ReadWrite<T: Copy> {
    value: UnsafeCell<T>,
}

impl<T: Copy> ReadWrite<T> {
    #[inline(always)]
    pub fn read(&self) -> {
        unsafe { read_volatile(self.value.get()) }
    }

    #[inline(always)]
    pub fn write(&self, val: T) {
        unsafe { write_volatile(self.value.get(), val) }
    }
}

#[repr(C)]
pub struct Pl011Regs {
    pub dr: ReadWrite<u32>,
    _reserved0: [u32; 5],

    pub fr: ReadOnly<u32>,
}

const PL011_UART_BASE: usize = 0x0900_0000;
const UART_FR_TXFF: u32 = 1 << 5;

pub struct Uart {
    base_addr: usize,
}

impl Uart {
    pub const fn new() -> {
        Self {
            base_addr: PL011_UART_BASE,
        }
    }

    #[inline(always)]
    fn regs(&self) -> &Pl011Regs {
        unsafe { &*(self.base_addr as *const Pl011Regs) }
    }

    pub fn putc(&self, byte: u8) {
        let regs = self.regs();

        while (regs.fr.read() & UART_FR_TXFF) != 0 {}

        regs.dr.write(byte as u32);
    }

    pub fn puts(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.putc(b'\r');
            }
            self.putc(byte);
        }
    }
}

imlp fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.puts(s);
        Ok(())
    }
}

#[inline(always)]
pub fn read_hardware_ticks() -> u64 {
    let ticks: u64;
    unsafe {
        core::arch::asm!(
            "isb",
            "mrs {0}, cntpct_el10",
            out(reg) ticks,
            options(nomem, nostack)
        );
    }
    ticks
}