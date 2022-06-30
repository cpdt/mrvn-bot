use std::cell::UnsafeCell;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct RingState {
    capacity: usize,
    buf: Box<UnsafeCell<[u8]>>,

    read: AtomicUsize,
    write: AtomicUsize,
}

pub struct Reader {
    state: Arc<RingState>,
}

pub struct Writer {
    state: Arc<RingState>,
}

unsafe impl Send for Reader {}
unsafe impl Sync for Reader {}

unsafe impl Send for Writer {}
unsafe impl Sync for Writer {}

pub fn nearest_ring_buffer(capacity: usize) -> (Reader, Writer) {
    let best_capacity = capacity.next_power_of_two().min(usize::MAX / 2);
    unsafe { unchecked_ring_buffer(best_capacity) }
}

/// # Safety
/// `capacity` must be a power of two, and less than `usize::MAX / 2`.
pub unsafe fn unchecked_ring_buffer(capacity: usize) -> (Reader, Writer) {
    let state = Arc::new(RingState {
        capacity,
        buf: into_boxed_unsafecell(vec![0u8; capacity].into_boxed_slice()),

        read: AtomicUsize::new(0),
        write: AtomicUsize::new(0),
    });

    let reader = Reader {
        state: state.clone(),
    };
    let writer = Writer { state };

    (reader, writer)
}

impl Reader {
    fn read_range(&self) -> Range<usize> {
        // todo: verify these orderings are needed
        let read = self.state.read.load(Ordering::SeqCst);
        let write = self.state.write.load(Ordering::SeqCst);

        // Buffer is empty if read == write
        if read == write {
            return Default::default();
        }

        let read_offset = read & (self.state.capacity - 1);
        let write_offset = write & (self.state.capacity - 1);

        let read_range = if read_offset >= write_offset {
            //                 V read this region
            // [DDD............DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD]
            //     ^-write     ^-read

            read_offset..self.state.capacity
        } else {
            //     V read this region
            // [...DDDDDDDDDDDD...........................................................]
            //     ^-read      ^-write

            read_offset..write_offset
        };

        debug_assert!(read_range.start < self.state.capacity);
        debug_assert!(read_range.end <= self.state.capacity);

        read_range
    }

    pub fn buffer(&self) -> &[u8] {
        let read_range = self.read_range();

        let buf_ptr = self.state.buf.get() as *const u8;

        // Safety: result pointer is guaranteed to be in range as long as
        // read_range.start < capacity, which will always be correct due to the mask.
        let slice_ptr = unsafe { buf_ptr.add(read_range.start) };

        // Safety: len is guaranteed to be in range as long as read_range.end <= capacity, which
        // will always be correct due to the mask.
        // Writer will never return an overlapping region until Reader.consume has been called,
        // since Reader only changes state.read and Writer will only increase state.write.
        unsafe { std::slice::from_raw_parts(slice_ptr, read_range.len()) }
    }

    pub fn consume(&mut self, len: usize) {
        assert!(len <= self.read_range().len());
        unsafe { self.consume_unchecked(len) };
    }

    /// # Safety
    /// `len` must be less than or equal to the length of the slice returned by [buffer].
    pub unsafe fn consume_unchecked(&mut self, len: usize) {
        // fetch_add wraps on overflow
        // todo: verify these orderings are needed
        self.state.read.fetch_add(len, Ordering::SeqCst);
    }
}

impl Writer {
    fn write_range(&self) -> Range<usize> {
        // todo: verify these orderings are needed
        let read = self.state.read.load(Ordering::SeqCst);
        let write = self.state.write.load(Ordering::SeqCst);

        let size = write.wrapping_sub(read);

        // Buffer is full if size == capacity
        if size == self.state.capacity {
            return Default::default();
        }

        let read_offset = read & (self.state.capacity - 1);
        let write_offset = write & (self.state.capacity - 1);

        let write_range = if read_offset <= write_offset {
            //                 V fill this region
            // [...DDDDDDDDDDDD...........................................................]
            //     ^-read      ^-write

            write_offset..self.state.capacity
        } else {
            //       V fill this region
            // [DDDDD......................................DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD]
            //       ^-write                               ^-read

            write_offset..read_offset
        };

        debug_assert!(write_range.start < self.state.capacity);
        debug_assert!(write_range.end <= self.state.capacity);

        write_range
    }

    pub fn buffer(&mut self) -> &mut [u8] {
        let write_range = self.write_range();

        let buf_ptr = self.state.buf.get() as *mut u8;

        // Safety: result pointer is guaranteed to be in range as long as
        // write_range.start < capacity, which will always be correct due to the mask.
        let slice_ptr = unsafe { buf_ptr.add(write_range.start) };

        // Safety: len is guaranteed to be in range as long as write_range.end <= capacity, which
        // will always be correct due to the mask.
        // Reader will never return an overlapping region until Writer.consume has been called,
        // since Writer only changes state.write and Reader will only increase state.read.
        unsafe { std::slice::from_raw_parts_mut(slice_ptr, write_range.len()) }
    }

    pub fn consume(&mut self, len: usize) {
        assert!(len <= self.write_range().len());
        unsafe { self.consume_unchecked(len) };
    }

    /// # Safety
    /// `len` must be less than or equal to the length of the slice returned by [buffer].
    pub unsafe fn consume_unchecked(&mut self, len: usize) {
        // fetch_add wraps on overflow
        // todo: verify these orderings are needed
        self.state.write.fetch_add(len, Ordering::SeqCst);
    }
}

fn into_boxed_unsafecell<T>(inp: Box<[T]>) -> Box<UnsafeCell<[T]>> {
    // Safety: UnsafeCell is #[repr(transparent)].
    unsafe { std::mem::transmute(inp) }
}
