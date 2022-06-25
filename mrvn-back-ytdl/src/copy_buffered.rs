use crate::ring_buffer::{ring_buffer, Reader, Writer};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

struct CopyBufferedBuffer {
    is_read_done: bool,
    reader: Reader,
    writer: Writer,
}

impl CopyBufferedBuffer {
    fn new(capacity: usize) -> Self {
        let (reader, writer) = ring_buffer(capacity);

        CopyBufferedBuffer {
            is_read_done: false,
            reader,
            writer,
        }
    }

    fn poll_copy<R, W>(
        &mut self,
        cx: &mut Context<'_>,
        mut reader: Pin<&mut R>,
        mut writer: Pin<&mut W>,
    ) -> Poll<io::Result<()>>
    where
        R: AsyncRead + ?Sized,
        W: AsyncWrite + ?Sized,
    {
        loop {
            let mut read_was_pending = false;
            let mut write_was_pending = false;

            let mut read_did_work = false;
            let mut write_did_work = false;

            // Read some data into the next contiguous region if the reader hasn't ended
            if !self.is_read_done {
                let write_buffer = self.writer.buffer();

                if !write_buffer.is_empty() {
                    read_did_work = true;
                    let mut buf = ReadBuf::new(write_buffer);

                    match reader.as_mut().poll_read(cx, &mut buf) {
                        Poll::Ready(Ok(_)) => {
                            let filled = buf.filled().len();

                            if filled == 0 {
                                self.is_read_done = true;
                            } else {
                                self.writer.consume(filled);
                            }
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => read_was_pending = true,
                    }
                }
            }

            // Write some data from the next contiguous region if possible
            {
                let read_buffer = self.reader.buffer();

                if !read_buffer.is_empty() {
                    write_did_work = true;
                    match writer.as_mut().poll_write(cx, read_buffer) {
                        Poll::Ready(Ok(0)) => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::WriteZero,
                                "write zero byte into writer",
                            )));
                        }
                        Poll::Ready(Ok(read_bytes)) => {
                            self.reader.consume(read_bytes);
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => write_was_pending = true,
                    }
                } else if self.is_read_done {
                    // We've seen EOF and all of the data has been written - flush it out and finish
                    // the transfer.
                    return match writer.as_mut().poll_flush(cx) {
                        Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                        Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                        Poll::Pending => Poll::Pending,
                    };
                }
            }

            debug_assert!(read_did_work || write_did_work);

            if (read_was_pending || !read_did_work) && (write_was_pending || !write_did_work) {
                return Poll::Pending;
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
struct CopyBuffered<'a, R: ?Sized, W: ?Sized> {
    reader: &'a mut R,
    writer: &'a mut W,
    buf: CopyBufferedBuffer,
}

impl<R, W> Future for CopyBuffered<'_, R, W>
where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;

        me.buf
            .poll_copy(cx, Pin::new(&mut *me.reader), Pin::new(&mut *me.writer))
    }
}

pub async fn copy_buffered<'a, R, W>(
    reader: &'a mut R,
    writer: &'a mut W,
    capacity: usize,
) -> io::Result<()>
where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
{
    CopyBuffered {
        reader,
        writer,
        buf: CopyBufferedBuffer::new(capacity),
    }
    .await
}
