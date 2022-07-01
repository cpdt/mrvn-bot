use crate::input::hls::media_file_stream::media_file_stream;
use crate::input::hls::media_segment_stream::segment_stream;
use bytes::Bytes;
use futures::Stream;
use tokio::io;

mod media_file_stream;
mod media_segment_stream;

pub fn hls_chunks(
    base_url: url::Url,
    initial_response: reqwest::Response,
    request_builder: reqwest::RequestBuilder,
) -> impl Stream<Item = io::Result<Bytes>> {
    media_file_stream(base_url, segment_stream(initial_response, request_builder))
}
