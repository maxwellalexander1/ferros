use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TelemtryFrame {
    pub seq_num: u32,
    pub timestamp_ticks: u64,
    pub die_temp_mc: u32,
    pub crc32: u32,
}

impl TelemtryFrame {
    pub const fn empty() -> Self {
        Self {
            seq_num: 0,
            timestamp_ticks: 0,
            die_temp_mc: 0,
            crc32: 0,
        }
    }

    pub fn new(seq_num: u32, timestamp_ticks: u64, die_temp_mc: u32) -> Self {
        let mut frame = Self {
            seq_num,
            timestamp_ticks,
            die_temp_mc,
            crc32: 0,
        };
        frame.crc32 = frame.compute_crc32();
        frame 
    }

    pub fn compute_crc32(&self) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;

        let mut feed_byte = |b : u8| {
            crc ^= b as u32;
            for i in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        };

        for &b in &self.seq_num.to_le_bytes(){
            feed_byte(b);
        }
        for &b in &self.timestamp_ticks.to_le_bytes() {
            feed_byte(b);
        }
        for &b in &self.die_temp_mc.to_le_bytes() {
            feed_byte(b);
        }

        !crc
    }

    pub fn is_valid(&self) -> bool {
        self.crc32 == self.compute_crc32()
    }
}

pub const RING_CAPACITY: usize = 16;

pub struct SpscRingBuffer {
    buffer: [UnsafeCell<TelemtryFrame>; RING_CAPACITY],
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for SpscRingBuffer {}

impl SpscRingBuffer {
    pub const fn new() -> Self {
        const EMPTY_SLOT: UnsafeCell<TelemtryFrame> = UnsafeCell::new(TelemtryFrame::empty());
        Self {
            buffer: [EMPTY_SLOT; RING_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, frame: TelemtryFrame) -> Result<(), TelemtryFrame> {
        let current_head = self.head.load(Ordering::Relaxed);
        let next_head = (current_head + 1) % RING_CAPACITY;

        if next_head == self.tail.load(Ordering::Acquire) {
            return Err(frame);
        }

        unsafe {
            *self.buffer[current_head].get() = frame;
        }

        self.head.store(next_head, Ordering::Release);
        Ok(())
    }

    pub fn pop(&self) -> Option<TelemtryFrame> {
        let current_tail = self.tail.load(Ordering::Relaxed);

        if current_tail == self.head.load(Ordering::Acquire) {
            return None;
        }

        let frame = unsafe { *self.buffer[current_tail].get() };

        let next_tail = (current_tail + 1) % RING_CAPACITY;
        self.tail.store(next_tail, Ordering::Release);
        Some(frame)
    }
}