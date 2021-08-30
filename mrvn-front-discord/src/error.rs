use serenity::model::prelude::*;

#[derive(Debug)]
pub enum Error {
    Serenity(serenity::Error),
    Backend(mrvn_back_ytdl::error::Error),

    UnknownCommand(String),
    MissingOption(&'static str),
    NoGuild,
    UnknownGuild(GuildId),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Serenity(err) => err.fmt(f),
            Error::Backend(err) => err.fmt(f),
            Error::UnknownCommand(command) => write!(f, "Received unknown command {}", command),
            Error::MissingOption(option) => write!(f, "Command was invoked without required {} option", option),
            Error::NoGuild => write!(f, "Command was not invoked from a guild"),
            Error::UnknownGuild(guild_id) => write!(f, "Unknown guild {}", guild_id),
        }
    }
}

impl std::error::Error for Error {}
