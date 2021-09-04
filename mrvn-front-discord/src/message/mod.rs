use serenity::model::prelude::*;

mod send_message;

pub use self::send_message::*;

#[derive(Debug, Clone)]
pub enum Message {
    Action(ActionMessage),
    Response(ResponseMessage),
}

impl Message {
    pub fn is_action(&self) -> bool {
        match self {
            Message::Action(_) => true,
            Message::Response(_) => false,
        }
    }

    pub fn to_string(&self, config: &crate::config::Config) -> String {
        match self {
            Message::Action(action) => action.to_string(config),
            Message::Response(response) => response.to_string(config),
        }
    }
}

/// Action messages have the possibility of being sent not directly as a response to a command
/// invocation. Only one action message is kept around in a guild at a time, old ones are deleted
/// when new ones are sent.
#[derive(Debug, Clone)]
pub enum ActionMessage {
    Playing {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
        user_id: UserId,
    },
    Finished {
        voice_channel_id: ChannelId,
    },
    NoSpeakersError {
        voice_channel_id: ChannelId,
    },
    UnknownError,
}

/// Response messages are always sent directly as a response to a command invocation.
#[derive(Debug, Clone)]
pub enum ResponseMessage {
    Queued {
        song_title: String,
        song_url: String,
    },
    QueuedNoSpeakers {
        song_title: String,
        song_url: String,
    },
    Paused {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
        user_id: UserId,
    },
    Skipped {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
        user_id: UserId,
    },
    SkipMoreVotesNeeded {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
        count: usize,
    },
    NoMatchingSongsError,
    NotInVoiceChannelError,
    SkipAlreadyVotedError {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
    },
    NothingIsQueuedError {
        voice_channel_id: ChannelId,
    },
    NothingIsPlayingError {
        voice_channel_id: ChannelId,
    },
    AlreadyPlayingError {
        voice_channel_id: ChannelId,
    }
}

impl ActionMessage {
    pub fn to_string(&self, config: &crate::config::Config) -> String {
        match self {
            ActionMessage::Playing { song_title, song_url, voice_channel_id, user_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                let user_id_string = user_id.0.to_string();
                config.get_message("action.playing", &[
                    ("song_title", song_title),
                    ("song_url", song_url),
                    ("voice_channel_id", &channel_id_string),
                    ("user_id", &user_id_string)
                ])
            }
            ActionMessage::Finished { voice_channel_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                config.get_message("action.finished", &[
                    ("voice_channel_id", &channel_id_string)
                ])
            }
            ActionMessage::NoSpeakersError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                config.get_message("action.noSpeakersError", &[
                    ("voice_channel_id", &channel_id_string)
                ])
            },
            ActionMessage::UnknownError => config.get_raw_message("action.unknownError").to_string(),
        }
    }
}

impl ResponseMessage {
    pub fn to_string(&self, config: &crate::config::Config) -> String {
        match self {
            ResponseMessage::Queued { song_title, song_url } => {
                config.get_message("response.queued", &[
                    ("song_title", song_title),
                    ("song_url", song_url),
                ])
            }
            ResponseMessage::QueuedNoSpeakers { song_title, song_url } => {
                config.get_message("response.queuedNoSpeakers", &[
                    ("song_title", song_title),
                    ("song_url", song_url),
                ])
            }
            ResponseMessage::Paused { song_title, song_url, voice_channel_id, user_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                let user_id_string = user_id.0.to_string();
                config.get_message("response.paused", &[
                    ("song_title", song_title),
                    ("song_url", song_url),
                    ("voice_channel_id", &channel_id_string),
                    ("user_id", &user_id_string),
                ])
            }
            ResponseMessage::Skipped { song_title, song_url, voice_channel_id, user_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                let user_id_string = user_id.0.to_string();
                config.get_message("response.skipped", &[
                    ("song_title", song_title),
                    ("song_url", song_url),
                    ("voice_channel_id", &channel_id_string),
                    ("user_id", &user_id_string),
                ])
            }
            ResponseMessage::SkipMoreVotesNeeded { song_title, song_url, voice_channel_id, count } => {
                let channel_id_string = voice_channel_id.0.to_string();
                if *count == 1 {
                    config.get_message("response.skipMoreVotesNeeded.singular", &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                    ])
                } else {
                    let count_string = count.to_string();
                    config.get_message("response.skipMoreVotesNeeded.plural", &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                        ("count", &count_string),
                    ])
                }
            }
            ResponseMessage::NoMatchingSongsError => {
                config.get_raw_message("response.noMatchingSongsError").to_string()
            }
            ResponseMessage::NotInVoiceChannelError => {
                config.get_raw_message("response.notInVoiceChannelError").to_string()
            }
            ResponseMessage::SkipAlreadyVotedError { song_title, song_url, voice_channel_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                config.get_message("response.skipAlreadyVotedError", &[
                    ("song_title", song_title),
                    ("song_url", song_url),
                    ("voice_channel_id", &channel_id_string),
                ])
            }
            ResponseMessage::NothingIsQueuedError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                config.get_message("response.nothingIsQueuedError", &[
                    ("voice_channel_id", &channel_id_string),
                ])
            }
            ResponseMessage::NothingIsPlayingError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                config.get_message("response.nothingIsPlayingError", &[
                    ("voice_channel_id", &channel_id_string),
                ])
            }
            ResponseMessage::AlreadyPlayingError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.0.to_string();
                config.get_message("response.alreadyPlayingError", &[
                    ("voice_channel_id", &channel_id_string),
                ])
            }
        }
    }
}
