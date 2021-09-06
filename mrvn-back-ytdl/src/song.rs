use crate::Error;
use serenity::model::prelude::UserId;

pub struct Song {
    pub metadata: SongMetadata,
    pub source: songbird::input::Input,
}

impl Song {
    pub async fn load(term: &str, user_id: UserId) -> Result<Song, Error> {
        let source = match url::Url::parse(term).is_ok() {
            true => songbird::input::ytdl(term).await,
            false => songbird::input::ytdl_search(term).await,
        }.map_err(Error::SongbirdInput)?;

        let title = source.metadata.title.clone().unwrap_or_default();
        let url = source.metadata.source_url.clone().unwrap_or_default();

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
