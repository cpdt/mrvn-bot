#[derive(Debug)]
pub enum Error {
    YoutubeDl(youtube_dl::Error),
    Songbird(songbird::input::error::Error),

    NoSongsFound,
    NoSongUrl,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::YoutubeDl(err) => err.fmt(f),
            Error::Songbird(err) => err.fmt(f),
            Error::NoSongsFound => write!(f, "No songs found"),
            Error::NoSongUrl => write!(f, "Missing song URL"),
        }
    }
}

impl std::error::Error for Error {}
