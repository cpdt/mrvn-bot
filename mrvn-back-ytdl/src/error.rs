#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Runtime(tokio::task::JoinError),
    Parse(serde_json::Error, String),
    Ytdl(String),
    Http(reqwest::Error),
    SongbirdJoin(songbird::error::JoinError),
    SongbirdControl(songbird::error::ControlError),
    Symphonia(symphonia::core::errors::Error),
    RubatoConstruction(rubato::ResamplerConstructionError),
    Rubato(rubato::ResampleError),
    UnsupportedUrl,
    NoDataProvided,
    NoTracks,
    ScanTimedOut,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Runtime(err) => err.fmt(f),
            Error::Parse(err, value) => write!(f, "{}: {}", err, value),
            Error::Ytdl(err) => write!(f, "Could not load media: {}", err),
            Error::Http(err) => err.fmt(f),
            Error::SongbirdJoin(err) => err.fmt(f),
            Error::SongbirdControl(err) => err.fmt(f),
            Error::Symphonia(err) => err.fmt(f),
            Error::RubatoConstruction(err) => err.fmt(f),
            Error::Rubato(err) => err.fmt(f),
            Error::UnsupportedUrl => write!(f, "Unsupported URL"),
            Error::NoDataProvided => write!(f, "No data provided"),
            Error::NoTracks => write!(f, "Media did not have any playable tracks"),
            Error::ScanTimedOut => write!(f, "Media scan timed out"),
        }
    }
}

impl std::error::Error for Error {}
