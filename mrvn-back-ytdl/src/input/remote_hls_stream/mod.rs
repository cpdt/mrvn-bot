use crate::input::remote_hls_stream::media_file_stream::media_file_stream;
use crate::input::remote_hls_stream::media_segment_stream::segment_stream;
use bytes::Bytes;
use futures::Stream;
use tokio::io;

mod media_file_stream;
mod media_segment_stream;

pub fn remote_hls_stream(
    initial_response: reqwest::Response,
    request_builder: reqwest::RequestBuilder,
) -> impl Stream<Item = io::Result<Bytes>> {
    media_file_stream(segment_stream(initial_response, request_builder))
}
