use crate::Error;
use serenity::model::prelude::UserId;

async fn load_info_for_term(term: String) -> Result<youtube_dl::SingleVideo, Error> {
    let query = match url::Url::parse(&term).is_ok() {
        true => youtube_dl::YoutubeDl::new(term),
        false => youtube_dl::YoutubeDl::search_for(&youtube_dl::SearchOptions::youtube(term).with_count(1)),
    };
    let res = tokio::task::spawn_blocking(move || query.run())
        .await
        .map_err(Error::Runtime)?
        .map_err(Error::YoutubeDl)?;

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
    pub metadata: SongMetadata,
    pub source: songbird::input::Input,
}

impl Song {
    pub async fn load(term: String, user_id: UserId) -> Result<Song, Error> {
        let info = load_info_for_term(term).await?;

        let title = info.title;
        let url = info.webpage_url.ok_or(Error::NoSongsFound)?;
        let source = songbird::ytdl(&url).await.map_err(Error::SongbirdInput)?;

        Ok(Song {
            metadata: SongMetadata {
                title,
                url,
                user_id,
            },
            source,
        })
    }
}

#[derive(Clone)]
pub struct SongMetadata {
    pub title: String,
    pub url: String,
    pub user_id: UserId,
}
