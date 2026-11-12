#![no_std]
#![no_main]

mod mmio;
mod ringbuf;

use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU32, Ordering};

use mmio::{read_hardware_ticks, Uart};
use ringbuf::{SpscRingBuffer, TelemtryFrame};

// Stack/BSS Initialization
global_asm!(
    ".section .text.boot",
    ".global _start",
    "_start:",
    "    mrs x0, mpidr_el1",
    "    and x0, x0, #0xFF",
    "    cbz x0, 1f",
    "0:  wfe",
    "    b 0b",
    "1:",
    "    ldr x0, =__stack_top",
    "    mov sp, x0",
    "    ldr x0, =__bss_start",
    "    ldr x1, =__bss_end",
    "2:  cmp x0, x1",
    "    b.ge 3f",
    "    str xzr, [x0], #8",
    "    b 2b",
    "3:",
    "    bl rust_firmware_main",
    "    b 0b",
);

static TELEMETRY_RING: SpscRingBuffer = SpscRingBuffer::new();
static FRAMES_VERIFIED: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone)]
pub struct Task {
    pub name: &'static str,
    pub step_fn: fn(u32),
}

pub struct MicroScheduler<const MAX_TASKS: usize> {
    tasks: [Option<Task>; MAX_TASKS],
    count: usize,
}

impl<const MAX_TASKS: usize> MicroScheduler<MAX_TASKS> {
    pub const fn new() -> Self {
        Self {
            tasks: [None; MAX_TASKS],
            count: 0,
        }
    }

    pub fn register_task(&mut self, name: &'static str, step_fn: fn(u32)) {
        assert!(self.count < MAX_TASKS, "Task table exhausted!");
        self.tasks[self.count] = Some(Task { name, step_fn });
        self.count += 1;
    }

    pub fn run_cycles(&self, rounds: u32) -> u64 {
        let start_ticks = read_hardware_ticks();
        for cycle in 0..rounds {
            for slot in 0..self.count {
                if let Some(task) = self.tasks[slot] {
                    (task.step_fn)(cycle);
                }
            }
        }
        read_hardware_ticks() - start_ticks
    }
}

fn task_sensor_acquire(seq: u32) {
    let ticks = read_hardware_ticks();
    let simulated_temp_mc = 42_500 + (seq * 125);
    let frame = TelemtryFrame::new(seq, ticks, simulated_temp_mc);

    TELEMETRY_RING
        .push(frame)
        .expect("Ring buffer overflow in sensor producer");
}

fn task_uart_telemetry(_seq: u32) {
    let mut uart = Uart::new();

    while let Some(frame) = TELEMETRY_RING.pop() {
        assert!(
            frame.is_valid(),
            "CRC-32 mismatch on frame seq={}",
            frame.seq_num
        );

        let whole_c = frame.die_temp_mc / 1000;
        let frac_c = (frame.die_temp_mc % 1000) / 10;

        let _ = writeln!(
            uart,
            "[TELEM #{:02}] Ticks={:<8} | DieTemp={}.{:02} C | CRC32={:#010X} [OK]",
            frame.seq_num, frame.timestamp_ticks, whole_c, frac_c, frame.crc32
        );

        FRAMES_VERIFIED.fetch_add(1, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_firmware_main() {
    let mut uart = Uart::new();

    let _ = writeln!(uart, "[FERROS] Bare-Metal #![no_std] Rust Firmware & Scheduler");

    let mut scheduler: MicroScheduler<4> = MicroScheduler::new();
    scheduler.register_task("sensor_isr_producer", task_sensor_acquire);
    scheduler.register_task("uart_telem_consumer", task_uart_telemetry);

    let total_rounds: u32 = 8;
    let elapsed_ticks = scheduler.run_cycles(total_rounds);

    let verified = FRAMES_VERIFIED.load(Ordering::Relaxed);
    let _ = writeln!(uart, "--------------------------------------------------------");
    let _ = writeln!(
        uart,
        "[PERF] Dispatched {} rounds ({} frames) in {} ticks ({} ticks/round)",
        total_rounds,
        verified,
        elapsed_ticks,
        elapsed_ticks / (total_rounds as u64)
    );

    assert_eq!(verified, total_rounds, "Dropped telemetry frames!");
    let _ = writeln!(
        uart,
        "[PASS] #![no_std] MMIO, Lock-Free RingBuffer & CRC-32 Self-Test OK"
    );

    qemu_exit_success();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut uart = Uart::new();
    let _ = writeln!(uart, "\n========================================================");
    let _ = writeln!(uart, "[FIRMWARE PANIC / WATCHDOG FAULT DUMP]");
    if let Some(location) = info.location() {
        let _ = writeln!(
            uart,
            "Location : {}:{}",
            location.file(),
            location.line()
        );
    }
    let _ = writeln!(uart, "Reason   : {}", info.message());
    let _ = writeln!(uart, "========================================================");

    loop {
        unsafe { asm!("wfe") };
    }
}

fn qemu_exit_success() -> ! {
    let block: [u64; 2] = [0x20026, 0];
    unsafe {
        asm!(
            "hlt #0xF000",
            in("x0") 0x18u64,
            in("x1") block.as_ptr() as u64,
            options(nostack)
        );
    }
    loop {
        unsafe { asm!("wfe") };
    }
}