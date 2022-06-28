use crate::ring_buffer::{ring_buffer, Reader, Writer};
use futures::channel::mpsc::{channel, Receiver, Sender};
use pin_project_lite::pin_project;
use std::io::Error;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::futures::Notified;

#[must_use]
#[repr(transparent)]
#[derive(Default)]
struct WakeOnDrop(Option<Waker>);

pin_project! {
    pub struct ReaderIo {
        reader: Reader,

        data_available_waker: Weak<Mutex<WakeOnDrop>>,
        space_available_waker: Arc<Mutex<WakeOnDrop>>,
    }
}

pin_project! {
    pub struct WriterIo {
        writer: Writer,

        data_available_waker: Option<Arc<Mutex<WakeOnDrop>>>,
        space_available_waker: Weak<Mutex<WakeOnDrop>>,
    }
}

pub fn ring_buffer_io(reserved: usize) -> (ReaderIo, WriterIo) {
    let (reader, writer) = ring_buffer(reserved);

    let data_available_waker = Arc::new(Mutex::new(WakeOnDrop(None)));
    let space_available_waker = Arc::new(Mutex::new(WakeOnDrop(None)));

    let data_available_waker_weak = Arc::downgrade(&data_available_waker);
    let space_available_waker_weak = Arc::downgrade(&space_available_waker);

    let reader_io = ReaderIo {
        reader,

        data_available_waker: data_available_waker_weak,
        space_available_waker,
    };
    let writer_io = WriterIo {
        writer,

        data_available_waker: Some(data_available_waker),
        space_available_waker: space_available_waker_weak,
    };

    (reader_io, writer_io)
}

impl ReaderIo {
    pub fn buffer(&self) -> &[u8] {
        self.reader.buffer()
    }
}

impl WriterIo {
    pub fn buffer(&mut self) -> &mut [u8] {
        self.writer.buffer()
    }
}

impl WakeOnDrop {
    fn wake(mut self) {
        if let Some(waker) = self.take() {
            waker.wake();
        }
    }

    fn take(&mut self) -> Option<Waker> {
        std::mem::take(&mut self.0)
    }
}

impl Drop for WakeOnDrop {
    fn drop(&mut self) {
        if let Some(waker) = self.take() {
            waker.wake();
        }
    }
}

impl AsyncRead for ReaderIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let src_buf = match self.as_mut().poll_fill_buf(cx) {
            Poll::Ready(Ok(buf)) => buf,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        };

        let unfilled = buf.initialize_unfilled();
        let take_len = src_buf.len().min(unfilled.len());
        unfilled[..take_len].copy_from_slice(&src_buf[..take_len]);
        buf.advance(take_len);

        log::trace!("Read {}/{}", take_len, src_buf.len());

        self.as_mut().consume(take_len);

        Poll::Ready(Ok(()))
    }
}

impl AsyncBufRead for ReaderIo {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<&[u8]>> {
        let me = self.project();

        let available_data = me.reader.buffer();
        let available_data = if available_data.is_empty() {
            let data_available_waker_mutex = match me.data_available_waker.upgrade() {
                Some(mutex) => mutex,
                None => {
                    // The writer has shut down, indicate that our end is complete since we've read
                    // all the data.
                    return Poll::Ready(Ok(Default::default()));
                }
            };
            let mut data_available_waker = data_available_waker_mutex.lock().unwrap();

            let available_data = me.reader.buffer();
            if available_data.is_empty() {
                log::trace!("No data to read, waiting for writer...");

                // Tell the writer to wake us when data becomes available.
                *data_available_waker = WakeOnDrop(Some(cx.waker().clone()));

                return Poll::Pending;
            }

            available_data
        } else {
            available_data
        };

        Poll::Ready(Ok(available_data))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let me = self.project();

        let space_available_waker = {
            let mut space_available_waker = me.space_available_waker.lock().unwrap();

            me.reader.consume(amt);

            std::mem::take(space_available_waker.deref_mut())
        };

        // If the writer was waiting for space to become available, wake it up now that we've
        // consumed space.
        space_available_waker.wake();
    }
}

impl AsyncWrite for WriterIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let me = self.project();

        let available_space = me.writer.buffer();
        let available_space = if available_space.is_empty() {
            let space_available_waker_mutex = match me.space_available_waker.upgrade() {
                Some(mutex) => mutex,
                None => {
                    // The reader has shut down, indicate that we can't write any more.
                    return Poll::Ready(Ok(0));
                }
            };
            let mut space_available_waker = space_available_waker_mutex.lock().unwrap();

            let available_space = me.writer.buffer();
            if available_space.is_empty() {
                log::trace!("No space to write, waiting for reader...");

                // Tell the reader to wake us when data becomes available.
                *space_available_waker = WakeOnDrop(Some(cx.waker().clone()));

                return Poll::Pending;
            }

            available_space
        } else {
            available_space
        };

        let take_len = available_space.len().min(buf.len());
        available_space[..take_len].copy_from_slice(&buf[..take_len]);

        let data_available_waker = {
            let mut data_available_waker = me
                .data_available_waker
                .as_ref()
                .expect("can't write after shutdown")
                .lock()
                .unwrap();

            me.writer.consume(take_len);

            std::mem::take(data_available_waker.deref_mut())
        };

        // If the reader was waiting for data to become available, wake it up now that we've
        // written something.
        data_available_waker.wake();

        Poll::Ready(Ok(take_len))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let mut me = self.project();

        let data_available_waker = {
            let data_available_waker_mutex =
                std::mem::take(me.data_available_waker).expect("can't shutdown twice");
            let mut data_available_waker = data_available_waker_mutex.lock().unwrap();

            std::mem::take(data_available_waker.deref_mut())
        };

        // If the reader was waiting for data to become available, wake it up now that we've
        // written EOF.
        data_available_waker.wake();

        Poll::Ready(Ok(()))
    }
}
