#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Runtime(tokio::task::JoinError),
    Parse(serde_json::Error),
    Http(reqwest::Error),
    SongbirdJoin(songbird::error::JoinError),
    SongbirdTrack(songbird::error::TrackError),
    UnsupportedUrl,
    NoDataProvided,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Runtime(err) => err.fmt(f),
            Error::Parse(err) => err.fmt(f),
            Error::Http(err) => err.fmt(f),
            Error::SongbirdJoin(err) => err.fmt(f),
            Error::SongbirdTrack(err) => err.fmt(f),
            Error::UnsupportedUrl => write!(f, "Unsupported URL"),
            Error::NoDataProvided => write!(f, "No data provided"),
        }
    }
}

impl std::error::Error for Error {}
