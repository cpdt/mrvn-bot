use crate::error::Error;

fn load_info_for_term(term: String) -> Result<youtube_dl::SingleVideo, Error> {
    let res = match url::Url::parse(&term).is_ok() {
        true => youtube_dl::YoutubeDl::new(term),
        false => youtube_dl::YoutubeDl::search_for(&youtube_dl::SearchOptions::youtube(term).with_count(1)),
    }.run().map_err(Error::YoutubeDl)?;

    match res {
        youtube_dl::YoutubeDlOutput::Playlist(playlist) => {
            playlist
                .entries
                .ok_or(Error::NoSongsFound)?
                .into_iter()
                .next()
                .ok_or(Error::NoSongsFound)
        }
        youtube_dl::YoutubeDlOutput::SingleVideo(video) => Ok(*video),
    }
}

pub struct Song {
    title: String,
    url: String,
    source: songbird::input::Input,
}

impl Song {
    pub async fn load(term: String) -> Result<Song, Error> {
        let info = load_info_for_term(term)?;

        let title = info.title;
        let url = info.webpage_url.ok_or(Error::NoSongsFound)?;
        let source = songbird::ytdl(&url).await.map_err(Error::SongbirdInput)?;

        Ok(Song {
            title,
            url,
            source,
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn source(self) -> songbird::input::Input {
        self.source
    }
}
