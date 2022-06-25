use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

struct RingBuffer {
    reserved: usize,
    buf: Box<UnsafeCell<[u8]>>,

    is_full: AtomicBool,
    pos: AtomicUsize,
    cap: AtomicUsize,
}

pub struct Reader {
    buffer: Arc<RingBuffer>,
}

pub struct Writer {
    buffer: Arc<RingBuffer>,
}

unsafe impl Send for Reader {}
unsafe impl Send for Writer {}

pub fn ring_buffer(reserved: usize) -> (Reader, Writer) {
    let buffer = Arc::new(RingBuffer {
        reserved,
        buf: into_boxed_unsafecell(vec![0u8; reserved].into_boxed_slice()),

        is_full: AtomicBool::new(false),
        pos: AtomicUsize::new(0),
        cap: AtomicUsize::new(0),
    });

    let reader = Reader {
        buffer: buffer.clone(),
    };
    let writer = Writer { buffer };

    (reader, writer)
}

impl Reader {
    pub fn buffer(&self) -> &[u8] {
        let is_full = self.buffer.is_full.load(Ordering::SeqCst);
        let pos = self.buffer.pos.load(Ordering::SeqCst);
        let cap = self.buffer.cap.load(Ordering::SeqCst);

        let read_range = if pos > cap || (is_full && pos == cap) {
            //                 V read this region
            // [DDD............DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD]
            //     ^-cap       ^-pos

            debug_assert!(pos < self.buffer.reserved);
            pos..self.buffer.reserved
        } else {
            //     V read this region
            // [...DDDDDDDDDDDD...........................................................]
            //     ^-pos       ^-cap

            debug_assert!(pos <= cap);
            pos..cap
        };

        debug_assert!(read_range.start < self.buffer.reserved);
        debug_assert!(read_range.end <= self.buffer.reserved);

        let buf_ptr = self.buffer.buf.get() as *const u8;

        // todo: safety
        let slice_ptr = unsafe { buf_ptr.add(read_range.start) };

        // todo: safety
        unsafe { std::slice::from_raw_parts(slice_ptr, read_range.len()) }
    }

    pub fn consume(&mut self, len: usize) {
        if len == 0 {
            return;
        }

        // we are the only writer to pos
        let is_full = self.buffer.is_full.load(Ordering::SeqCst);
        let pos = self.buffer.pos.load(Ordering::SeqCst);
        let cap = self.buffer.cap.load(Ordering::SeqCst);

        let new_pos = pos + len;
        let new_pos = if pos > cap || (is_full && pos == cap) {
            debug_assert!(new_pos <= self.buffer.reserved);
            if new_pos == self.buffer.reserved {
                0
            } else {
                new_pos
            }
        } else {
            debug_assert!(new_pos <= cap);
            new_pos
        };

        self.buffer.pos.store(new_pos, Ordering::SeqCst);
        self.buffer.is_full.store(false, Ordering::SeqCst);
    }
}

impl Writer {
    pub fn buffer(&mut self) -> &mut [u8] {
        if self.buffer.is_full.load(Ordering::SeqCst) {
            return Default::default();
        }

        let pos = self.buffer.pos.load(Ordering::SeqCst);
        let cap = self.buffer.cap.load(Ordering::SeqCst);

        let write_range = if pos <= cap {
            //                 V fill this region
            // [...DDDDDDDDDDDD...........................................................]
            //     ^-pos       ^-cap

            cap..self.buffer.reserved
        } else {
            //       V fill this region
            // [DDDDD......................................DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD]
            //       ^-cap                                 ^-pos

            cap..pos
        };

        debug_assert!(write_range.start < self.buffer.reserved);
        debug_assert!(write_range.end <= self.buffer.reserved);

        let buf_ptr = self.buffer.buf.get() as *mut u8;

        // todo: safety
        let slice_ptr = unsafe { buf_ptr.add(write_range.start) };

        // todo: safety
        unsafe { std::slice::from_raw_parts_mut(slice_ptr, write_range.len()) }
    }

    pub fn consume(&mut self, len: usize) {
        if len == 0 || self.buffer.is_full.load(Ordering::SeqCst) {
            return;
        }

        // we are the only writer to cap
        let pos = self.buffer.pos.load(Ordering::SeqCst);
        let cap = self.buffer.cap.load(Ordering::SeqCst);

        let new_cap = cap + len;
        let new_cap = if pos <= cap {
            debug_assert!(new_cap <= self.buffer.reserved);
            if new_cap == self.buffer.reserved {
                0
            } else {
                new_cap
            }
        } else {
            debug_assert!(new_cap <= pos);
            new_cap
        };

        if new_cap == pos {
            self.buffer.is_full.store(true, Ordering::SeqCst);
        }
        self.buffer.cap.store(new_cap, Ordering::SeqCst);
    }
}

fn into_boxed_unsafecell<T>(inp: Box<[T]>) -> Box<UnsafeCell<[T]>> {
    // Safety: UnsafeCell is #[repr(transparent)].
    unsafe { std::mem::transmute(inp) }
}
