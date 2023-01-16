#[derive(Debug)]
pub enum Error {
    Serenity(serenity::Error),
    Backend(mrvn_back_ytdl::Error),

    UnknownCommand(String),
    NoGuild,
    ModelPlayingSpeakerNotDesync,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Serenity(err) => err.fmt(f),
            Error::Backend(err) => err.fmt(f),
            Error::UnknownCommand(command) => write!(f, "Received unknown command {}", command),
            Error::NoGuild => write!(f, "Command was not invoked from a guild"),
            Error::ModelPlayingSpeakerNotDesync => write!(
                f,
                "Out of sync: model says song is playing, but the speaker disagrees"
            ),
        }
    }
}

impl std::error::Error for Error {}
