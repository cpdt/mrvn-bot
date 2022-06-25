use crate::copy_buffered::copy_buffered;
use crate::input::{remote_file_stream, remote_hls_stream};
use crate::{Error, HTTP_CLIENT};
use futures::future::{AbortHandle, Abortable};
use futures::pin_mut;
use serenity::model::prelude::UserId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::process::{Child, Command, Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::io::StreamReader;
use uuid::Uuid;

const DEFAULT_FFMPEG_ARGS: &[&str] = &[
    "-vn",
    "-f",
    "s16le",
    "-ac",
    "2",
    "-ar",
    "48000",
    "-acodec",
    "pcm_f32le",
    "-",
];

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
    pub ffmpeg_name: &'s str,
    pub ffmpeg_args: &'s [String],
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
        let source = StreamingSource::new(config, parsed_download_url, request_builder).await?;

        Ok(songbird::input::Input::new(
            true,
            songbird::input::Reader::Extension(Box::new(source)),
            songbird::input::Codec::FloatPcm,
            songbird::input::Container::Raw,
            None,
        ))
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

struct StreamingSource {
    content_length: Option<u64>,
    abort_download: AbortHandle,
    ffmpeg: Child,
    ffmpeg_out: std::process::ChildStdout,
}

impl StreamingSource {
    pub async fn new(
        config: &PlayConfig<'_>,
        request_url: url::Url,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<Self, Error> {
        let buffer_capacity_bytes = config.buffer_capacity_kb * 1024;

        let initial_response = request_builder
            .try_clone()
            .unwrap()
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(Error::Http)?;

        let content_length = initial_response.content_length();

        let is_hls_stream = request_url.path().ends_with(".m3u8")
            || request_url.path().ends_with(".m3u")
            || initial_response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .map(|header| {
                    header == "application/vnd.apple.mpegurl" || header == "audio/mpegurl"
                })
                .unwrap_or(false);

        // Reduce the buffer capacity if it's under the content length, to avoid allocating
        // unnecessary memory.
        let buffer_capacity_bytes = match content_length {
            Some(content_length) => {
                buffer_capacity_bytes.min((content_length as usize).next_power_of_two())
            }
            None => buffer_capacity_bytes,
        };

        // Construct a writer for ffmpeg
        let mut ffmpeg = Command::new(config.ffmpeg_name)
            .args(config.ffmpeg_args)
            .args(&["-i", "-"])
            .args(DEFAULT_FFMPEG_ARGS)
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(Error::Io)?;

        let mut ffmpeg_in = tokio::process::ChildStdin::from_std(ffmpeg.stdin.take().unwrap())
            .map_err(Error::Io)?;

        let (abort_download, abort_registration) = AbortHandle::new_pair();
        tokio::spawn(Abortable::new(
            async move {
                // Start streaming data from the remote and copying it to FFMPEG.
                // HLS URLs (i.e HTTP livestreams) can't be streamed just by downloading a file, so
                // they use a separate implementation.
                let maybe_err = if is_hls_stream {
                    let reader =
                        StreamReader::new(remote_hls_stream(initial_response, request_builder));
                    pin_mut!(reader);
                    copy_buffered(&mut reader, &mut ffmpeg_in, buffer_capacity_bytes).await
                } else {
                    let reader =
                        StreamReader::new(remote_file_stream(initial_response, request_builder));
                    pin_mut!(reader);
                    copy_buffered(&mut reader, &mut ffmpeg_in, buffer_capacity_bytes).await
                };

                if let Err(why) = maybe_err {
                    log::error!("Error while streaming data: {}", why);
                }
            },
            abort_registration,
        ));

        let ffmpeg_out = ffmpeg.stdout.take().unwrap();
        Ok(StreamingSource {
            content_length,
            abort_download,
            ffmpeg,
            ffmpeg_out,
        })
    }
}

impl Drop for StreamingSource {
    fn drop(&mut self) {
        self.abort_download.abort();
        if let Err(why) = self.ffmpeg.kill() {
            log::error!("Error stopping transcoder: {}", why);
        }
    }
}

impl songbird::input::reader::MediaSource for StreamingSource {
    fn is_seekable(&self) -> bool {
        false
    }

    fn len(&self) -> Option<u64> {
        self.content_length
    }
}

impl std::io::Read for StreamingSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.ffmpeg_out.read(buf)
    }
}

impl std::io::Seek for StreamingSource {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        panic!("Attempting to seek on non-seekable streaming source");
    }
}
