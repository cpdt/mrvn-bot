use async_stream::try_stream;
use bytes::Bytes;
use futures::Stream;
use tokio::io;

pub fn remote_file_stream(
    initial_response: reqwest::Response,
    request_builder: reqwest::RequestBuilder,
) -> impl Stream<Item = io::Result<Bytes>> {
    try_stream! {
        let content_length = initial_response.content_length();
        let mut response = initial_response;
        let mut received_bytes = 0;

        loop {
            let mut received_this_request = 0;
            for await bytes_maybe in response.bytes_stream() {
                let bytes = match bytes_maybe {
                    Ok(bytes) => bytes,
                    Err(why) => {
                        log::warn!("Error while receiving data: {}", why);
                        break;
                    }
                };

                received_this_request += bytes.len() as u64;

                yield bytes;
            }
            received_bytes += received_this_request;

            // Some remotes close the request after a certain timeout. To avoid just ending
            // playback when this happens, under certain circumstances we can restart the
            // request with a Range header set.
            // We only keep requesting if:
            //  - The initial request had a Content-Length header set, so we know when to stop.
            //  - We haven't received the amount of data we were meant to get.
            //  - We did not receive an empty response in this request. This ensures we don't
            //    get into an infinite request loop.
            let content_length = match content_length {
                Some(length) => length,
                None => break,
            };
            if received_bytes >= content_length || received_this_request == 0 {
                break;
            }

            response = request_builder
                .try_clone()
                .unwrap()
                .header(
                    reqwest::header::RANGE,
                    format!("bytes={}-{}", received_bytes, content_length),
                )
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        }
    }
}
