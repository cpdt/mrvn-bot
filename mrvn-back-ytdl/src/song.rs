use crate::formats::MpegTsReader;
use crate::input::{hls_chunks, remote_file_chunks};
use crate::ring_buffer_io::ring_buffer_io;
use crate::source::{AbortOnDropSource, DecodedPcmSource, OpusPassthroughSource};
use crate::{Error, HTTP_CLIENT};
use futures::future::{AbortHandle, Abortable};
use futures::{future, pin_mut, TryStreamExt};
use serenity::model::prelude::UserId;
use songbird::constants::SAMPLE_RATE_RAW;
use songbird::input::codec::OpusDecoderState;
use songbird::input::{Codec, Container, Input, Metadata, Reader};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;
use std::process::Stdio;
use std::time::Duration;
use symphonia::core::audio::Layout;
use symphonia::core::codecs::{
    Decoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_NULL, CODEC_TYPE_OPUS,
};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::codecs::AacDecoder;
use tokio::io::{copy_buf, AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;
use tokio_util::io::{StreamReader, SyncIoBridge};
use uuid::Uuid;

pub struct Song {
    pub metadata: SongMetadata,
    download_url: String,
    http_headers: Vec<(String, String)>,
}

pub struct PlayConfig<'s> {
    pub search_prefix: &'s str,
    pub host_blocklist: &'s [String],
    pub ytdl_name: &'s str,
    pub ytdl_args: &'s [String],
    pub scan_timeout_secs: f64,
    pub buffer_capacity_kb: usize,
}

#[derive(serde::Deserialize)]
struct YtdlOutput {
    pub title: String,
    pub webpage_url: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub http_headers: HashMap<String, String>,
    pub duration: Option<f64>,
}

fn parse_ytdl_line(line: &str, user_id: UserId) -> Result<Song, Error> {
    let trimmed_line = line.trim();
    if trimmed_line.starts_with("ERROR:") {
        return Err(Error::UnsupportedUrl);
    }

    let value: YtdlOutput = serde_json::from_str(trimmed_line)
        .map_err(|err| Error::Parse(err, trimmed_line.to_string()))?;

    Ok(Song {
        metadata: SongMetadata {
            id: Uuid::new_v4(),
            title: value.title,
            url: value.webpage_url,
            thumbnail_url: value.thumbnail,
            duration_seconds: if value.duration == Some(0.) {
                None
            } else {
                value.duration
            },
            user_id,
        },
        download_url: value.url.to_string(),
        http_headers: value
            .http_headers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    })
}

impl Song {
    pub async fn load(
        term: &str,
        user_id: UserId,
        config: &PlayConfig<'_>,
    ) -> Result<Vec<Song>, Error> {
        let ytdl_url = match url::Url::parse(term) {
            Ok(url) => {
                if let Some(host_str) = url.host_str() {
                    // Ensure the resolved host isn't in the blocklist
                    if config
                        .host_blocklist
                        .iter()
                        .any(|domain| host_str.contains(domain))
                    {
                        return Err(Error::UnsupportedUrl);
                    }
                }

                Cow::Borrowed(term)
            }
            Err(_) => Cow::Owned(format!("{}:{}", config.search_prefix, &term)),
        };

        let mut ytdl = TokioCommand::new(config.ytdl_name)
            .args(config.ytdl_args)
            .args(&[
                "--dump-json",
                "--ignore-config",
                "--no-warnings",
                ytdl_url.as_ref(),
                "-o",
                "-",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(Error::Io)?;
        let mut lines = BufReader::new(ytdl.stderr.take().unwrap()).lines();

        let mut songs = Vec::new();
        while let Some(line) = lines.next_line().await.map_err(Error::Io)? {
            songs.push(parse_ytdl_line(&line, user_id)?);
        }

        Ok(songs)
    }

    pub async fn fetch_one(
        webpage_url: &str,
        user_id: UserId,
        config: &PlayConfig<'_>,
    ) -> Result<Song, Error> {
        let mut ytdl = TokioCommand::new(config.ytdl_name)
            .args(config.ytdl_args)
            .args(&[
                "--dump-json",
                "--ignore-config",
                "--no-warnings",
                "--no-playlist",
                webpage_url,
                "-o",
                "-",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(Error::Io)?;
        let first_line = BufReader::new(ytdl.stderr.take().unwrap())
            .lines()
            .next_line()
            .await
            .map_err(Error::Io)?
            .ok_or(Error::UnsupportedUrl)?;

        parse_ytdl_line(&first_line, user_id)
    }

    pub async fn get_input(
        &self,
        config: &PlayConfig<'_>,
    ) -> Result<songbird::input::Input, Error> {
        // The cached download URL might have become invalid since fetching it. We assume it's fine
        // but fetch a new one from youtube-dl if playback fails.
        match self.get_input_no_retry(config).await {
            Ok(input) => Ok(input),
            Err(why) => {
                log::error!(
                    "Error opening stream to play {}: {}",
                    &self.metadata.url,
                    why
                );
                let refetch_song =
                    Song::fetch_one(&self.metadata.url, self.metadata.user_id, config).await?;
                refetch_song.get_input_no_retry(config).await
            }
        }
    }

    async fn get_input_no_retry(
        &self,
        config: &PlayConfig<'_>,
    ) -> Result<songbird::input::Input, Error> {
        let parsed_download_url =
            url::Url::parse(&self.download_url).map_err(|_| Error::UnsupportedUrl)?;

        // Start streaming data from the remote
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, value) in &self.http_headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }

        let request_builder = HTTP_CLIENT.get(&self.download_url).headers(headers);
        create_source(config, parsed_download_url, request_builder).await
    }
}

#[derive(Clone)]
pub struct SongMetadata {
    pub id: Uuid,
    pub title: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<f64>,
    pub user_id: UserId,
}

async fn create_source(
    config: &PlayConfig<'_>,
    request_url: url::Url,
    request_builder: reqwest::RequestBuilder,
) -> Result<songbird::input::Input, Error> {
    let buffer_capacity_bytes = config.buffer_capacity_kb * 1024;
    let scan_timeout = Duration::from_secs_f64(config.scan_timeout_secs);

    let initial_response = request_builder
        .try_clone()
        .unwrap()
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(Error::Http)?;

    let maybe_extension = request_url
        .path_segments()
        .and_then(|segments| segments.last())
        .and_then(|segment| segment.rfind('.').map(|idx| (segment, idx)))
        .map(|(segment, idx)| &segment[(idx + 1)..]);

    let maybe_mime_type = initial_response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|val| val.to_str().ok());

    let is_mpeg_stream = maybe_extension == Some("m3u8")
        || maybe_extension == Some("m3u")
        || maybe_mime_type == Some("application/vnd.apple.mpegurl")
        || maybe_mime_type == Some("audio/mpegurl");

    let mut hint = Hint::new();
    maybe_extension.map(|extension| hint.with_extension(extension));
    maybe_mime_type.map(|mime_type| hint.mime_type(mime_type));

    // Start streaming chunks from the remote
    let (abort_stream, abort_registration) = AbortHandle::new_pair();
    let (ring_reader, mut ring_writer) = ring_buffer_io(buffer_capacity_bytes);
    tokio::spawn(Abortable::new(
        async move {
            let maybe_err = if is_mpeg_stream {
                let stream = hls_chunks(request_url, initial_response, request_builder);
                let reader =
                    StreamReader::new(stream.try_filter(|chunk| future::ready(!chunk.is_empty())));
                pin_mut!(reader);

                copy_buf(&mut reader, &mut ring_writer).await
            } else {
                let stream = remote_file_chunks(initial_response, request_builder);
                let reader =
                    StreamReader::new(stream.try_filter(|chunk| future::ready(!chunk.is_empty())));
                pin_mut!(reader);

                copy_buf(&mut reader, &mut ring_writer).await
            };

            if let Err(why) = maybe_err {
                log::warn!("Error while streaming data: {}", why);
            }
        },
        abort_registration,
    ));

    let sync_reader = SyncIoBridge::new(ring_reader);

    // Symphonia does not detect MPEG-TS streams, so we must use a separate branch if we are hinted
    // to have one of those.
    if is_mpeg_stream {
        create_mpeg_stream_source(sync_reader, abort_stream, scan_timeout).await
    } else {
        create_file_source(sync_reader, hint, abort_stream, scan_timeout).await
    }
}

async fn create_mpeg_stream_source(
    reader: impl Read + Sync + Send + 'static,
    abort: AbortHandle,
    scan_timeout: Duration,
) -> Result<songbird::input::Input, Error> {
    let scan_future = tokio::task::spawn_blocking(move || {
        let source = ReadOnlySource::new(reader);
        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());

        MpegTsReader::try_new(stream, &FormatOptions::default())
    });
    let maybe_format = timeout(scan_timeout, scan_future).await;

    // timeout returns Err if the scan times out
    let maybe_format = maybe_format.map_err(|_| Error::ScanTimedOut)?;

    // spawn_blocking returns Err if the inner function panics, propagate this to our thread
    let maybe_format = maybe_format.unwrap();

    let format = maybe_format.map_err(Error::Symphonia)?;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec == CODEC_TYPE_AAC)
        .ok_or(Error::NoTracks)?;

    let track_id = track.id;

    // Assume the track is stereo if it's not mono. DecodedPcmSource will strip any extra channels.
    let is_stereo = !matches!(track.codec_params.channel_layout, Some(Layout::Mono));

    let metadata = Metadata {
        channels: track
            .codec_params
            .channels
            .map(|channels| channels.count() as u8),
        sample_rate: track.codec_params.sample_rate,

        ..Default::default()
    };

    let decoder = AacDecoder::try_new(&track.codec_params, &DecoderOptions::default())
        .map_err(Error::Symphonia)?;

    let source = DecodedPcmSource::new(Box::new(format), Box::new(decoder), track_id, is_stereo)?;
    let source = AbortOnDropSource::new(source, abort);

    Ok(Input::new(
        is_stereo,
        Reader::Extension(Box::new(source)),
        Codec::FloatPcm,
        Container::Raw,
        Some(metadata),
    ))
}

async fn create_file_source(
    reader: impl Read + Sync + Send + 'static,
    hint: Hint,
    abort: AbortHandle,
    scan_timeout: Duration,
) -> Result<Input, Error> {
    let scan_future = tokio::task::spawn_blocking(move || {
        let source = ReadOnlySource::new(reader);
        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());

        symphonia::default::get_probe().format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,

                ..Default::default()
            },
            &MetadataOptions::default(),
        )
    });
    let maybe_probe_result = timeout(scan_timeout, scan_future).await;

    // timeout returns Err if the scan times out
    let maybe_probe_result = maybe_probe_result.map_err(|_| Error::ScanTimedOut)?;

    // spawn_blocking returns Err if the inner function panics, propagate this to our thread
    let maybe_probe_result = maybe_probe_result.unwrap();

    let probe_result = maybe_probe_result.map_err(Error::Symphonia)?;
    let format = probe_result.format;

    // Look for any tracks that can be passed through. This allows us to skip re-encoding if the
    // stream is in the format Discord expects.
    // The data must be Opus encoded with a 48kHz sample rate and 20ms long frames.
    // todo: how can we check frame length?
    for track in format.tracks() {
        let track_id = track.id;

        // Assume the track is stereo if it's not mono. This might break for tracks with more channels.
        let is_stereo = !matches!(track.codec_params.channel_layout, Some(Layout::Mono));

        let can_pass_through = track.codec_params.codec == CODEC_TYPE_OPUS
            && track.codec_params.sample_rate == Some(SAMPLE_RATE_RAW as u32);

        if can_pass_through {
            let metadata = Metadata {
                channels: track
                    .codec_params
                    .channels
                    .map(|channels| channels.count() as u8),
                sample_rate: track.codec_params.sample_rate,

                ..Default::default()
            };

            let source = OpusPassthroughSource::new(format, track_id);

            return Ok(Input::new(
                is_stereo,
                Reader::Extension(Box::new(source)),
                Codec::Opus(OpusDecoderState::new().unwrap()),
                Container::Dca { first_frame: 0 },
                Some(metadata),
            ));
        }
    }

    // If we are here, we will need to pick a track and decode it to PCM.
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(Error::NoTracks)?;

    let track_id = track.id;

    // Assume the track is stereo if it's not mono. DecodedPcmSource will strip any extra channels.
    let is_stereo = !matches!(track.codec_params.channel_layout, Some(Layout::Mono));

    let metadata = Metadata {
        channels: track
            .codec_params
            .channels
            .map(|channels| channels.count() as u8),
        sample_rate: Some(SAMPLE_RATE_RAW as u32),

        ..Default::default()
    };

    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(Error::Symphonia)?;

    let source = DecodedPcmSource::new(format, decoder, track_id, is_stereo)?;
    let source = AbortOnDropSource::new(source, abort);

    Ok(Input::new(
        is_stereo,
        Reader::Extension(Box::new(source)),
        Codec::FloatPcm,
        Container::Raw,
        Some(metadata),
    ))
}
