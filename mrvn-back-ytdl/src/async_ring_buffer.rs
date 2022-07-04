use crate::ring_buffer::{nearest_ring_buffer, Reader, Writer};
use futures::task::AtomicWaker;
use futures::{AsyncBufRead, AsyncRead, AsyncWrite};
use pin_project_lite::pin_project;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

// todo: store ring buffer state in the same Arc to reduce allocations

struct State {
    is_reader_closed: AtomicBool,
    is_writer_closed: AtomicBool,

    data_available_waker: AtomicWaker,
    space_available_waker: AtomicWaker,
}

struct AsyncReaderState(Arc<State>);

struct AsyncWriterState(Arc<State>);

pin_project! {
    pub struct AsyncReader {
        reader: Reader,
        state: AsyncReaderState,
    }
}

pin_project! {
    pub struct AsyncWriter {
        writer: Writer,
        state: AsyncWriterState,
    }
}

pub fn nearest_async_ring_buffer(capacity: usize) -> (AsyncReader, AsyncWriter) {
    let (reader, writer) = nearest_ring_buffer(capacity);

    let state = Arc::new(State {
        is_reader_closed: AtomicBool::new(false),
        is_writer_closed: AtomicBool::new(false),

        data_available_waker: AtomicWaker::new(),
        space_available_waker: AtomicWaker::new(),
    });

    let reader = AsyncReader {
        reader,
        state: AsyncReaderState(state.clone()),
    };
    let writer = AsyncWriter {
        writer,
        state: AsyncWriterState(state),
    };

    (reader, writer)
}

impl Deref for AsyncReaderState {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl Drop for AsyncReaderState {
    fn drop(&mut self) {
        self.0.is_reader_closed.store(true, Ordering::Release);
        self.0.space_available_waker.wake();
    }
}

impl Deref for AsyncWriterState {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl Drop for AsyncWriterState {
    fn drop(&mut self) {
        self.0.is_writer_closed.store(true, Ordering::Release);
        self.0.data_available_waker.wake();
    }
}

impl AsyncRead for AsyncReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let src_buf = match self.as_mut().poll_fill_buf(cx) {
            Poll::Ready(Ok(buf)) => buf,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        };

        let len = src_buf.len().min(buf.len());
        buf[..len].copy_from_slice(&src_buf[..len]);

        self.as_mut().consume(len);

        Poll::Ready(Ok(len))
    }
}

impl AsyncBufRead for AsyncReader {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<&[u8]>> {
        let me = self.project();
        let buf = me.reader.buffer();

        // If no data is available, ask the writer to wake us when it writes something.
        let buf = if buf.is_empty() {
            me.state.data_available_waker.register(cx.waker());

            // If the writer is closed, we've now read everything we could.
            // This can't be Relaxed, since we must observe a true value after being woken due to
            // the writer closing.
            if me.state.is_writer_closed.load(Ordering::Acquire) {
                return Poll::Ready(Ok(Default::default()));
            }

            // There is a possibility of a race condition where we read an empty buffer but
            // something was written before we set the waker, so we're not going to be woken up.
            // To avoid this we must double-check that the buffer is still empty now.
            let buf = me.reader.buffer();
            if buf.is_empty() {
                // Still empty, writer will wake us when data is available.
                return Poll::Pending;
            } else {
                // Data is ready, remove the waker to avoid unnecessary work.
                me.state.data_available_waker.take();

                buf
            }
        } else {
            buf
        };

        Poll::Ready(Ok(buf))
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        self.reader.consume(amt);

        // Wake the writer if it was waiting for space.
        self.state.space_available_waker.wake();
    }
}

impl AsyncWrite for AsyncWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let me = self.project();

        // If the reader is closed, there is no point writing anything.
        // This can't be Relaxed, since we must observe a true value after being woken due to the
        // reader closing.
        if me.state.is_reader_closed.load(Ordering::Acquire) {
            return Poll::Ready(Ok(0));
        }

        let dest_buf = me.writer.buffer();

        // If no space is available, ask the reader to wake us when it reads something.
        let dest_buf = if dest_buf.is_empty() {
            me.state.space_available_waker.register(cx.waker());

            // There is a possibility of a race condition where the reader closed, or we read an
            // empty buffer but something was read, before we set the waker - so we're not going
            // to be woken up. To avoid this we must double-check that if the buffer is open and
            // still empty now.
            if me.state.is_reader_closed.load(Ordering::Acquire) {
                return Poll::Ready(Ok(0));
            }
            let dest_buf = me.writer.buffer();
            if dest_buf.is_empty() {
                // Still empty, reader will wake us when data is available.
                return Poll::Pending;
            } else {
                // Data is ready, remove the waker to avoid unnecessary work.
                me.state.space_available_waker.take();

                dest_buf
            }
        } else {
            dest_buf
        };

        let len = dest_buf.len().min(buf.len());
        dest_buf[..len].copy_from_slice(&buf[..len]);

        me.writer.consume(len);

        // Wake the reader if it was waiting for data.
        me.state.data_available_waker.wake();

        Poll::Ready(Ok(len))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.writer.buffer().is_empty() {
            return Poll::Ready(Ok(()));
        }

        // Wait for more data to be read so we can check again if the buffer is empty.
        self.state.space_available_waker.register(cx.waker());

        // There is a possibility of a race condition where we read an empty buffer but
        // something was read before we set the waker, so we're not going to be woken up.
        // To avoid this we must double-check that the buffer is still empty now.
        if self.writer.buffer().is_empty() {
            self.state.space_available_waker.take();
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let was_closed = self.state.is_writer_closed.swap(true, Ordering::Release);
        if was_closed {
            panic!("attempted to close an AsyncWriter twice");
        }

        self.state.data_available_waker.wake();
        Poll::Ready(Ok(()))
    }
}
