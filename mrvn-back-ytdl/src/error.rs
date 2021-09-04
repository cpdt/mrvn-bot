#[derive(Debug)]
pub enum Error {
    Runtime(tokio::task::JoinError),
    YoutubeDl(youtube_dl::Error),
    SongbirdInput(songbird::input::error::Error),
    SongbirdJoin(songbird::error::JoinError),
    SongbirdTrack(songbird::error::TrackError),

    NoSongsFound,
    NoSongUrl,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Runtime(err) => err.fmt(f),
            Error::YoutubeDl(err) => err.fmt(f),
            Error::SongbirdInput(err) => err.fmt(f),
            Error::SongbirdJoin(err) => err.fmt(f),
            Error::SongbirdTrack(err) => err.fmt(f),
            Error::NoSongsFound => write!(f, "No songs found"),
            Error::NoSongUrl => write!(f, "Missing song URL"),
        }
    }
}

impl std::error::Error for Error {}
