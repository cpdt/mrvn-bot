use futures::future::AbortHandle;
use songbird::input::reader::MediaSource;
use std::io::{Read, Seek, SeekFrom};

pub struct AbortOnDrop(pub AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct AbortOnDropSource<S> {
    inner: S,
    _abort: AbortOnDrop,
}

impl<S> AbortOnDropSource<S> {
    pub fn new(inner: S, abort: AbortOnDrop) -> Self {
        AbortOnDropSource {
            inner,
            _abort: abort,
        }
    }
}

impl<S: MediaSource> MediaSource for AbortOnDropSource<S> {
    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.inner.byte_len()
    }
}

impl<S: Read> Read for AbortOnDropSource<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<S: Seek> Seek for AbortOnDropSource<S> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}
