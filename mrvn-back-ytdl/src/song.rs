use crate::input::{hls_chunks, remote_file_chunks};
use crate::{Error, HTTP_CLIENT};
use futures::{future, TryStreamExt};
use serenity::async_trait;
use serenity::model::prelude::UserId;
use songbird::input::core::io::MediaSource;
use songbird::input::{AsyncAdapterStream, AsyncMediaSource, AudioStream, Input, LiveInput};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};
use symphonia::core::probe::Hint;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncSeek, BufReader, ReadBuf};
use tokio::process::Command as TokioCommand;
use tokio_util::io::StreamReader;
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
    pub buffer_capacity_kb: usize,
}

#[derive(serde::Deserialize)]
struct YtdlOutput {
    pub title: String,
    pub fulltitle: Option<String>,
    pub description: Option<String>,
    pub extractor: String,
    pub webpage_url: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub http_headers: HashMap<String, String>,
    pub duration: Option<f64>,
}

fn parse_ytdl_line(line: &str, user_id: UserId) -> Result<Song, Error> {
    let trimmed_line = line.trim();
    if let Some(error) = trimmed_line.strip_prefix("ERROR: ") {
        return Err(Error::Ytdl(error.to_string()));
    }

    let value: YtdlOutput = serde_json::from_str(trimmed_line)
        .map_err(|err| Error::Parse(err, trimmed_line.to_string()))?;

    // Twitch stream extractor puts the stream title as the description for some reason
    let title = match &value.extractor as &str {
        "twitch:stream" => value.description,
        _ => value.fulltitle,
    };
    let title = title.unwrap_or(value.title);

    Ok(Song {
        metadata: SongMetadata {
            id: Uuid::new_v4(),
            title,
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
            .args([
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
            .args([
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
) -> Result<Input, Error> {
    let buffer_capacity_bytes = config.buffer_capacity_kb * 1024;

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

    if is_mpeg_stream {
        // todo: use hint of file linked in m3u8
        // m3u8 stream will probably contain MPEG-TS files
        hint.with_extension("ts");
        hint.mime_type("video/mp2t");
    } else {
        maybe_extension.map(|extension| hint.with_extension(extension));
        maybe_mime_type.map(|mime_type| hint.mime_type(mime_type));
    }

    // Start streaming chunks from the remote
    let adapter_stream = if is_mpeg_stream {
        let stream = hls_chunks(request_url, initial_response, request_builder);
        let reader = StreamReader::new(stream.try_filter(|chunk| future::ready(!chunk.is_empty())));
        AsyncAdapterStream::new(
            Box::new(AsyncReader::new(Box::pin(reader))),
            buffer_capacity_bytes,
        )
    } else {
        let stream = remote_file_chunks(initial_response, request_builder);
        let reader = StreamReader::new(stream.try_filter(|chunk| future::ready(!chunk.is_empty())));
        AsyncAdapterStream::new(
            Box::new(AsyncReader::new(Box::pin(reader))),
            buffer_capacity_bytes,
        )
    };

    let audio_stream = AudioStream {
        input: Box::new(adapter_stream) as Box<dyn MediaSource>,
        hint: Some(hint),
    };
    Ok(Input::Live(LiveInput::Raw(audio_stream), None))
}

struct AsyncReader<T> {
    inner: Pin<Box<T>>,
}

impl<T> AsyncReader<T> {
    fn new(inner: Pin<Box<T>>) -> Self {
        AsyncReader { inner }
    }
}

impl<T> AsyncRead for AsyncReader<T>
where
    T: AsyncRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

impl<T> AsyncSeek for AsyncReader<T> {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Err(std::io::ErrorKind::Unsupported.into()))
    }
}

#[async_trait]
impl<T> AsyncMediaSource for AsyncReader<T>
where
    T: AsyncRead + Send + Sync,
{
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        None
    }
}
