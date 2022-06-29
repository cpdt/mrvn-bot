use async_stream::try_stream;
use futures::Stream;
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

pub fn segment_stream(
    initial_response: reqwest::Response,
    request_builder: reqwest::RequestBuilder,
) -> impl Stream<Item = io::Result<m3u8_rs::MediaSegment>> {
    try_stream! {
        let mut request_instant = Instant::now();
        let mut response = initial_response;
        let mut last_seen_sequence = None;

        loop {
            let response_bytes = response.bytes()
                .await
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

            // Filter segments that start less than three target durations from the end of the file
            let min_start_secs = playlist_duration_secs - media_playlist.target_duration * 3.;
            let filtered_segments = sequenced_segments
                .scan(0., |start_time, (segment_sequence, segment)| {
                    let this_start_time = *start_time;
                    *start_time += segment.duration;
                    Some((segment_sequence, segment, this_start_time))
                })
                .filter(|(_, _, start_time)| *start_time < min_start_secs);

            // Filter segments that we've already seen
            let mut filtered_segments = filtered_segments
                .filter(move |(segment_sequence, _, _)| match last_seen_sequence {
                    Some(last_seen_sequence) => *segment_sequence > last_seen_sequence,
                    None => true,
                });

            let refresh_instant = match filtered_segments.next() {
                Some((first_segment_sequence, first_segment, _)) => {
                    // Prepare time for when we should refresh, at least.
                    // From https://datatracker.ietf.org/doc/html/rfc8216#section-6.3.4 -
                    //    When a client loads a Playlist file for the first time or reloads a
                    //    Playlist file and finds that it has changed since the last time it
                    //    was loaded, the client MUST wait for at least the target duration
                    //    before attempting to reload the Playlist file again, measured from
                    //    the last time the client began loading the Playlist file.
                    let refresh_instant = request_instant + Duration::from_secs_f32(media_playlist.target_duration);

                    if let Some(last_seen_sequence) = last_seen_sequence {
                        if last_seen_sequence + 1 < first_segment_sequence {
                            log::warn!("Discontinuity in HLS stream (sequence {} to {})", last_seen_sequence, first_segment_sequence);
                        }
                    }

                    yield first_segment;
                    last_seen_sequence = Some(first_segment_sequence);

                    for (segment_sequence, segment, segment_start_secs) in filtered_segments {
                        // Due to the yield points, there could have been any amount of time since
                        // the previous segment was emitted and this segment is about to be, e.g.
                        // if playback is paused.
                        // To avoid wasted work down the line, skip this and remaining segments
                        // if we can be certain they will have expired.
                        let segment_expiry_time = request_instant + Duration::from_secs_f32(segment_start_secs + playlist_duration_secs);
                        if Instant::now() > segment_expiry_time {
                            break;
                        }

                        yield segment;
                        last_seen_sequence = Some(segment_sequence);
                    }

                    refresh_instant
                }
                None => {
                    // No new segments.
                    // From https://datatracker.ietf.org/doc/html/rfc8216#section-6.3.4 -
                    //    If the client reloads a Playlist file and finds that it has not
                    //    changed, then it MUST wait for a period of one-half the target
                    //    duration before retrying.
                    request_instant + Duration::from_secs_f32(media_playlist.target_duration / 2.)
                }
            };

            if media_playlist.end_list {
                break;
            }

            let now = Instant::now();
            if refresh_instant > now {
                tokio::time::sleep_until(refresh_instant).await;
            }

            // Refresh the data again and continue
            log::trace!("Fetching segment list");
            request_instant = Instant::now();
            response = request_builder
                .try_clone()
                .unwrap()
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        }
    }
}
