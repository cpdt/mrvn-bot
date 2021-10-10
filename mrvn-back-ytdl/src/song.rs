use crate::Error;
use serenity::model::prelude::UserId;
use tokio::process::Command as TokioCommand;
use tokio::io::{BufReader, AsyncBufReadExt, AsyncWriteExt};
use std::process::{Stdio, Command, Child};
use std::borrow::Cow;
use std::collections::HashMap;
use futures::future::{AbortHandle, Abortable};
use std::io::SeekFrom;

const DEFAULT_FFMPEG_ARGS: &'static [&'static str] = &[
    "-vn",
    "-f",
    "s16le",
    "-ac",
    "2",
    "-ar",
    "48000",
    "-acodec",
    "pcm_f32le",
    "-"
];

pub struct Song {
    pub metadata: SongMetadata,
    download_url: String,
    http_headers: Vec<(String, String)>,
}

pub struct PlayConfig<'s> {
    pub request_retry_times: u32,
    pub search_prefix: &'s str,
    pub host_blocklist: &'s [String],
    pub ytdl_name: &'s str,
    pub ytdl_args: &'s [String],
    pub ffmpeg_name: &'s str,
    pub ffmpeg_args: &'s [String],
}

#[derive(serde::Deserialize)]
struct YtdlOutput {
    pub title: String,
    pub webpage_url: String,
    pub url: String,
    pub http_headers: HashMap<String, String>,
}

impl Song {
    pub async fn load(term: &str, user_id: UserId, config: &PlayConfig<'_>) -> Result<Vec<Song>, Error> {
        let ytdl_url = match url::Url::parse(term) {
            Ok(url) => {
                if let Some(host_str) = url.host_str() {
                    // Ensure the resolved host isn't in the blocklist
                    if config.host_blocklist.iter().any(|domain| host_str.contains(domain)) {
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
                "-"
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(Error::Io)?;
        let mut lines = BufReader::new(ytdl.stderr.take().unwrap()).lines();

        let mut songs = Vec::new();
        while let Some(line) = lines.next_line().await.map_err(Error::Io)? {
            if line.starts_with("ERROR:") {
                return Err(Error::UnsupportedUrl);
            }

            let value: YtdlOutput = serde_json::from_str(&line).map_err(Error::Parse)?;

            songs.push(Song {
                metadata: SongMetadata {
                    title: value.title.to_string(),
                    url: value.webpage_url.to_string(),
                    user_id,
                },
                download_url: value.url.to_string(),
                http_headers: value.http_headers
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect()
            })
        }

        Ok(songs)
    }

    pub async fn get_input(&self, config: &PlayConfig<'_>) -> Result<songbird::input::Input, Error> {
        // If this is a livestream, directly call FFMPEG instead of doing the download step ourself
        if self.download_url.ends_with(".m3u8") {
            let http_headers: String = self.http_headers
                .iter()
                .map(|(key, value)| format!("{}: {}\r\n", key, value))
                .collect();

            let ffmpeg = Command::new(config.ffmpeg_name)
                .args(config.ffmpeg_args)
                .args(&[
                    "-headers",
                    &http_headers,
                    "-i",
                    &self.download_url,
                ])
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
            headers.insert(reqwest::header::HeaderName::from_bytes(key.as_bytes()).unwrap(), value.parse().unwrap());
        }

        lazy_static::lazy_static! {
            static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder().build().unwrap();
        }

        let request_builder = HTTP_CLIENT
            .get(&self.download_url)
            .headers(headers);
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
    pub title: String,
    pub url: String,
    pub user_id: UserId,
}

struct StreamingSource {
    content_length: Option<u64>,
    abort_download: AbortHandle,
    ffmpeg: Child,
    ffmpeg_out: std::process::ChildStdout,
}

async fn send_request(request_builder: reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
    let response = request_builder.send().await.map_err(Error::Http)?;
    match response.content_length() {
        Some(0) => Err(Error::NoDataProvided),
        _ => Ok(response),
    }
}

impl StreamingSource {
    pub async fn new(config: &PlayConfig<'_>, request_builder: reqwest::RequestBuilder) -> Result<Self, Error> {
        let initial_response = tryhard::retry_fn(|| send_request(request_builder.try_clone().unwrap()))
            .retries(config.request_retry_times)
            .await?;
        let content_length = initial_response.content_length();
        let (abort_download, abort_registration) = AbortHandle::new_pair();

        let mut ffmpeg = Command::new(config.ffmpeg_name)
            .args(config.ffmpeg_args)
            .args(&[
                "-i",
                "-",
            ])
            .args(DEFAULT_FFMPEG_ARGS)
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(Error::Io)?;

        let mut ffmpeg_in = tokio::process::ChildStdin::from_std(ffmpeg.stdin.take().unwrap()).map_err(Error::Io)?;

        let stream_future = async move {
            // Keep trying to load data until we've loaded the max possible
            let mut response = initial_response;
            let mut received_bytes = 0;

            loop {
                // Pipe the data to FFMPEG
                let received_this_request = pipe_response(&mut response, &mut ffmpeg_in)
                    .await
                    .map_err(Error::Io)?;
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
                    None => break
                };
                if received_bytes >= content_length {
                    break;
                }
                if received_this_request == 0 {
                    break;
                }
                response = request_builder
                    .try_clone()
                    .unwrap()
                    .header(reqwest::header::RANGE, format!("bytes={}-{}", received_bytes, content_length))
                    .send()
                    .await
                    .map_err(Error::Http)?;
            }

            Result::<(), Error>::Ok(())
        };
        tokio::spawn(Abortable::new(async move {
            if let Err(why) = stream_future.await {
                log::error!("Error while streaming data: {}", why);
            }
        }, abort_registration));

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

async fn pipe_chunk(mut chunk: &[u8], stream: &mut tokio::process::ChildStdin) -> std::io::Result<u64> {
    let mut total_written_bytes = 0;
    while !chunk.is_empty() {
        let written_bytes = stream.write(chunk).await?;
        total_written_bytes += written_bytes as u64;

        if written_bytes == 0 {
            log::warn!("Skipping {} bytes while streaming", chunk.len());
            break;
        }

        chunk = &chunk[written_bytes..];
    }
    Ok(total_written_bytes)
}

async fn pipe_response(response: &mut reqwest::Response, stream: &mut tokio::process::ChildStdin) -> std::io::Result<u64> {
    let mut total_received_bytes = 0;
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(why) => {
                log::warn!("Error while reading stream: {}", why);
                break;
            }
        };

        total_received_bytes += chunk.len() as u64;
        pipe_chunk(chunk.as_ref(), stream).await?;
    }
    Ok(total_received_bytes)
}
