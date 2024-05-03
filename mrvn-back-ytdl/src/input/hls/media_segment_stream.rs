use async_stream::try_stream;
use futures::{future, stream, Stream, StreamExt, TryStreamExt};
use m3u8_rs::parse_media_playlist_res;
use std::fmt::{Debug, Display, Formatter};
use tokio::io;
use tokio::time::{Duration, Instant};

#[derive(Debug)]
struct MediaPlaylistParseError;

impl Display for MediaPlaylistParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse media playlist")
    }
}

impl std::error::Error for MediaPlaylistParseError {}

struct SegmentData {
    segment: m3u8_rs::MediaSegment,
    sequence: u64,
    expiry: Instant,
}

fn segment_list_stream(
    initial_response: reqwest::Response,
    request_builder: reqwest::RequestBuilder,
) -> impl Stream<Item = io::Result<Vec<SegmentData>>> {
    try_stream! {
        let mut initial_response = Some(initial_response);
        let mut last_seen_sequence = None;

        loop {
            let request_instant = Instant::now();
            let response = match initial_response.take() {
                Some(response) => response,
                None => {
                    request_builder
                        .try_clone()
                        .unwrap()
                        .send()
                        .await
                        .and_then(reqwest::Response::error_for_status)
                        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
                }
            };

            let response_bytes = response.bytes().await
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            let media_playlist = parse_media_playlist_res(&response_bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, MediaPlaylistParseError))?;

            let playlist_duration_secs: f32 = media_playlist.segments
                .iter()
                .map(|segment| segment.duration)
                .sum();

            let media_sequence = media_playlist.media_sequence;
            let sequenced_segments = media_playlist.segments
                .into_iter()
                .enumerate()
                .map(|(segment_index, segment)| (media_sequence + segment_index as u64, segment));

            let timed_segments = sequenced_segments
                .scan(0., |start_time, (segment_sequence, segment)| {
                    let this_start_time = *start_time;
                    *start_time += segment.duration;
                    Some((segment_sequence, segment, this_start_time))
                });

            // Filter segments:
            //  - If this isn't the first playlist, filter segments we have already seen
            //  - If this is the first playlist, filter all segments until the first one that ends
            //    before three target durations from the end of the file
            //    ^ only if the playlist hasn't ended (to support non-live streams)
            let min_end_secs = playlist_duration_secs - media_playlist.target_duration as f32 * 3.;
            let filtered_segments = timed_segments
                .filter(move |(segment_sequence, segment, segment_start_time)| match last_seen_sequence {
                    Some(last_seen_sequence) => *segment_sequence > last_seen_sequence,
                    None => media_playlist.end_list || segment_start_time + segment.duration >= min_end_secs,
                });

            let segments_with_expiry_time: Vec<_> = filtered_segments
                .map(|(sequence, segment, segment_start_secs)| {
                    SegmentData {
                        segment,
                        sequence,
                        expiry: request_instant + Duration::from_secs_f32(segment_start_secs + playlist_duration_secs)
                    }
                })
                .collect();

            let refresh_instant = match (segments_with_expiry_time.first(), segments_with_expiry_time.last()) {
                (Some(first_segment), Some(last_segment)) => {
                    if let Some(last_seen_sequence) = last_seen_sequence {
                        if last_seen_sequence + 1 < first_segment.sequence {
                            log::warn!("Discontinuity in HLS stream (sequence {} to {})", last_seen_sequence, first_segment.sequence);
                        }
                    }

                    last_seen_sequence = Some(last_segment.sequence);

                    yield segments_with_expiry_time;

                    // From https://datatracker.ietf.org/doc/html/rfc8216#section-6.3.4 -
                    //    When a client loads a Playlist file for the first time or reloads a
                    //    Playlist file and finds that it has changed since the last time it
                    //    was loaded, the client MUST wait for at least the target duration
                    //    before attempting to reload the Playlist file again, measured from
                    //    the last time the client began loading the Playlist file.
                    request_instant + Duration::from_secs(media_playlist.target_duration)
                }
                _ => {
                    // No new segments.
                    yield vec![];

                    // From https://datatracker.ietf.org/doc/html/rfc8216#section-6.3.4 -
                    //    If the client reloads a Playlist file and finds that it has not
                    //    changed, then it MUST wait for a period of one-half the target
                    //    duration before retrying.
                    request_instant + Duration::from_secs_f32(media_playlist.target_duration as f32 / 2.)
                }
            };

            if media_playlist.end_list {
                break;
            }

            tokio::time::sleep_until(refresh_instant).await;
        }
    }
}

pub fn segment_stream(
    initial_response: reqwest::Response,
    request_builder: reqwest::RequestBuilder,
) -> impl Stream<Item = io::Result<m3u8_rs::MediaSegment>> {
    segment_list_stream(initial_response, request_builder)
        .map(|segments| Ok(future::ready(segments)))
        .try_buffered(1)
        .map_ok(|segments| stream::iter(segments).map(io::Result::Ok))
        .try_flatten()
        .try_filter_map(|segment_data| async move {
            let now = Instant::now();
            if now > segment_data.expiry {
                log::warn!(
                    "Ignoring segment {} since it has expired (-{} secs)",
                    segment_data.sequence,
                    (now - segment_data.expiry).as_secs_f64()
                );
                return Ok(None);
            }

            Ok(Some(segment_data.segment))
        })
}
