use crate::copy_buffered::copy_watermark;
use crate::Error;
use async_stream::try_stream;
use futures::future::{AbortHandle, Abortable};
use futures::{pin_mut, TryStreamExt};
use serenity::model::prelude::UserId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{ErrorKind, SeekFrom};
use std::process::{Child, Command, Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::compat::FuturesAsyncReadCompatExt;
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
    pub buffer_watermark_kb: usize,
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
        // If this is a livestream, directly call FFMPEG instead of doing the download step ourself
        let parsed_download_url =
            url::Url::parse(&self.download_url).map_err(|_| Error::UnsupportedUrl)?;
        if parsed_download_url.path().ends_with(".m3u8") {
            let http_headers: String = self
                .http_headers
                .iter()
                .map(|(key, value)| format!("{}: {}\r\n", key, value))
                .collect();

            let ffmpeg = Command::new(config.ffmpeg_name)
                .args(config.ffmpeg_args)
                .args(&["-headers", &http_headers, "-i", &self.download_url])
                .args(DEFAULT_FFMPEG_ARGS)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(Error::Io)?;
            return Ok(songbird::input::Input::new(
                true,
                vec![ffmpeg].into(),
                songbird::input::Codec::FloatPcm,
                songbird::input::Container::Raw,
                None,
            ));
        }

        // Start streaming data from the remote
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, value) in &self.http_headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }

        lazy_static::lazy_static! {
            static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder().build().unwrap();
        }

        let request_builder = HTTP_CLIENT.get(&self.download_url).headers(headers);
        let source = StreamingSource::new(config, request_builder).await?;

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
        request_builder: reqwest::RequestBuilder,
    ) -> Result<Self, Error> {
        let buffer_capacity_bytes = config.buffer_capacity_kb * 1024;
        let buffer_watermark_bytes = config.buffer_watermark_kb * 1024;

        let initial_response = request_builder
            .try_clone()
            .unwrap()
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(Error::Http)?;

        let content_length = initial_response.content_length();

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

        // Copy the incoming data to ffmpeg
        let (abort_download, abort_registration) = AbortHandle::new_pair();
        tokio::spawn(Abortable::new(
            async move {
                // Construct a reader from that request and any necessary continuing requests
                let content_stream = try_stream! {
                    let mut response = initial_response;
                    let mut received_bytes = 0;

                    loop {
                        let mut received_this_request = 0;
                        for await bytes_maybe in response.bytes_stream() {
                            let bytes = bytes_maybe.map_err(|err| std::io::Error::new(ErrorKind::Other, err))?;
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
                            .map_err(|err| std::io::Error::new(ErrorKind::Other, err))?;
                    }
                };
                pin_mut!(content_stream);
                let content_read = content_stream.into_async_read();
                let mut content_read = content_read.compat();

                if let Err(why) = copy_watermark(
                    &mut content_read,
                    &mut ffmpeg_in,
                    buffer_watermark_bytes,
                    buffer_capacity_bytes,
                )
                .await
                {
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
