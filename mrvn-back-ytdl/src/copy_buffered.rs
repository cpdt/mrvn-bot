use std::future::Future;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum State {
    Filling,
    Copying,
    ReadDone,
}

#[derive(Debug)]
struct CopyWatermarkBuffer {
    buf: Box<[MaybeUninit<u8>]>,
    state: State,
    is_full: bool,
    pos: usize,
    cap: usize,
    init: usize,
    watermark: usize,
}

impl CopyWatermarkBuffer {
    fn new(watermark: usize, capacity: usize) -> Self {
        CopyWatermarkBuffer {
            buf: vec![MaybeUninit::uninit(); capacity].into_boxed_slice(),
            state: State::Filling,
            is_full: false,
            pos: 0,
            cap: 0,
            init: 0,
            watermark,
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
        let mut is_first_iter = false;

        loop {
            let mut read_was_pending = false;
            let mut write_was_pending = false;

            let mut read_did_work = false;
            let mut write_did_work = false;

            // Read some data into the next contiguous region if the reader hasn't ended
            if !read_was_pending
                && self.state != State::ReadDone
                && !self.is_full
                && self.pos <= self.cap
            {
                //                 V fill this region
                // [...DDDDDDDDDDDD...........................................................]
                //     ^-pos       ^-cap

                let mut buf = ReadBuf::uninit(&mut self.buf[self.pos..]);
                unsafe { buf.assume_init(self.init - self.pos) };
                buf.set_filled(self.cap - self.pos);

                match reader.as_mut().poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(_)) => {
                        let initial_cap = self.cap;
                        self.cap = buf.filled().len() + self.pos;
                        self.init = buf.initialized().len() + self.pos;

                        debug_assert!(self.cap >= self.pos);
                        debug_assert!(self.cap <= self.buf.len());

                        if initial_cap == self.cap {
                            self.state = State::ReadDone;
                        } else if self.cap == self.buf.len() {
                            self.is_full = self.pos == 0;
                            self.cap = 0;
                        }

                        if self.state == State::Filling
                            && (self.is_full || (self.pos == 0 && self.cap >= self.watermark))
                        {
                            self.state = State::Copying;
                        }
                    }
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                    Poll::Pending => read_was_pending = true,
                }

                read_did_work = true;
            }

            if !read_was_pending
                && self.state != State::ReadDone
                && !self.is_full
                && self.pos > self.cap
            {
                //       V fill this region
                // [DDDDD......................................DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD]
                //       ^-cap                                 ^-pos

                let mut buf = ReadBuf::uninit(&mut self.buf[..self.pos]);
                unsafe { buf.assume_init(self.pos) };
                buf.set_filled(self.cap);

                match reader.as_mut().poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(_)) => {
                        let initial_cap = self.cap;
                        self.cap = buf.filled().len();

                        debug_assert!(self.cap <= self.pos);

                        if initial_cap == self.cap {
                            self.state = State::ReadDone;
                        } else if self.cap == self.pos {
                            self.is_full = true;
                        }
                    }
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                    Poll::Pending => read_was_pending = true,
                }

                read_did_work = true;
            }

            // Write some data from the next contiguous region if if in a valid state
            if self.state != State::Filling {
                if is_first_iter
                    && self.state != State::ReadDone
                    && self.pos == self.cap
                    && !self.is_full
                {
                    log::warn!("Ran out of buffered data, playback may stutter");
                }

                if !write_was_pending && self.pos > self.cap
                    || (self.is_full && self.pos == self.cap)
                {
                    //                 V read this region
                    // [DDD............DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD]
                    //     ^-cap       ^-pos

                    let mut buf = ReadBuf::uninit(&mut self.buf[self.pos..]);
                    unsafe { buf.assume_init(self.init - self.pos) };

                    match writer.as_mut().poll_write(cx, buf.initialized_mut()) {
                        Poll::Ready(Ok(0)) => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::WriteZero,
                                "write zero byte into writer",
                            )));
                        }
                        Poll::Ready(Ok(read_bytes)) => {
                            debug_assert!(read_bytes <= self.buf.len() - self.pos);

                            self.is_full = false;
                            self.pos += read_bytes;

                            debug_assert!(self.pos <= self.buf.len());

                            if self.pos == self.buf.len() {
                                self.pos = 0;
                            }
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => write_was_pending = true,
                    }

                    write_did_work = true;
                }

                if !write_was_pending && self.pos < self.cap {
                    //     V read this region
                    // [...DDDDDDDDDDDD...........................................................]
                    //     ^-pos       ^-cap

                    let mut buf = ReadBuf::uninit(&mut self.buf[self.pos..self.cap]);
                    unsafe { buf.assume_init(self.cap - self.pos) };

                    match writer.as_mut().poll_write(cx, buf.initialized_mut()) {
                        Poll::Ready(Ok(0)) => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::WriteZero,
                                "write zero byte into writer",
                            )));
                        }
                        Poll::Ready(Ok(read_bytes)) => {
                            debug_assert!(read_bytes <= self.cap - self.pos);

                            self.is_full = false;
                            self.pos += read_bytes;

                            debug_assert!(self.pos <= self.cap);
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => write_was_pending = true,
                    }

                    write_did_work = true;
                }

                // If we've seen EOF and all of the data has been written, flush out the data and
                // finish the transfer.
                if self.state == State::ReadDone && !self.is_full && self.pos == self.cap {
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

            is_first_iter = false;
        }
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
struct CopyWatermark<'a, R: ?Sized, W: ?Sized> {
    reader: &'a mut R,
    writer: &'a mut W,
    buf: CopyWatermarkBuffer,
}

impl<R, W> Future for CopyWatermark<'_, R, W>
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

pub async fn copy_watermark<'a, R, W>(
    reader: &'a mut R,
    writer: &'a mut W,
    watermark: usize,
    capacity: usize,
) -> io::Result<()>
where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
{
    CopyWatermark {
        reader,
        writer,
        buf: CopyWatermarkBuffer::new(watermark, capacity),
    }
    .await
}
