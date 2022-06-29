use crate::HTTP_CLIENT;
use bytes::Bytes;
use futures::{FutureExt, Stream, StreamExt, TryStreamExt};
use m3u8_rs::Key;
use std::fmt::{Display, Formatter};
use tokio::io;

#[derive(Debug)]
struct EncryptionNotSupportedError;

impl Display for EncryptionNotSupportedError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "encryption is not supported")
    }
}

impl std::error::Error for EncryptionNotSupportedError {}

pub fn media_file_stream(
    segments: impl Stream<Item = io::Result<m3u8_rs::MediaSegment>> + Send + 'static,
) -> impl Stream<Item = io::Result<Bytes>> {
    // This looks like a mess, but roughly we're:
    //  1. Building a request for each incoming segment and sending it.
    //  2. Buffering several requests at a time, so we can initiate many of the requests for a
    //     segment at the same time.
    //  3. Ignore requests that failed. This can happen due to various causes but we should only
    //     need to halt if the segments stream errors.
    //  4. Start streaming chunks from each request, again ignoring errors.
    // The result is a plain stream of byte chunks.
    segments
        .and_then(|segment| async move {
            if let Some(Key { method, .. }) = &segment.key {
                if method != "NONE" {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        EncryptionNotSupportedError,
                    ));
                }
            }

            // todo: support range requests
            // todo: support relative uri
            // todo: support encryption

            log::trace!("Fetching segment");
            let builder = HTTP_CLIENT.get(segment.uri);
            Ok(builder.send().map(Ok))
        })
        .try_buffered(5)
        .try_filter_map(|maybe_response| async move {
            match maybe_response {
                Ok(response) => Ok(Some(response)),
                Err(why) => {
                    log::warn!("Error while loading playlist segment: {}", why);
                    Ok(None)
                }
            }
        })
        .map_ok(|response| {
            response
                .bytes_stream()
                .filter_map(|maybe_chunk| async move {
                    match maybe_chunk {
                        Ok(chunk) => Some(Ok(chunk)),
                        Err(why) => {
                            log::warn!("Error while streaming playlist segment: {}", why);
                            None
                        }
                    }
                })
        })
        .try_flatten()
}
