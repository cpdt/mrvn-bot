use serenity::all::CreateEmbed;
use crate::message::time_bar::format_time_bar;
use serenity::model::prelude::*;

mod action_updater;
mod default_action_delegate;
mod message_delegate;
mod send_message;
pub mod time_bar;

pub use self::action_updater::*;
pub use self::message_delegate::*;
pub use self::send_message::*;

pub enum Message {
    Action {
        message: ActionMessage,
        voice_channel: ChannelId,
        delegate: Option<Box<dyn ActionDelegate>>,
    },
    Response {
        message: ResponseMessage,
        delegate: Option<Box<dyn ResponseDelegate>>,
    },
}

impl Message {
    pub fn is_action(&self) -> bool {
        match self {
            Message::Action { .. } => true,
            Message::Response { .. } => false,
        }
    }

    /*pub fn create_embed<'e>(
        &self,
        embed: &'e mut serenity::builder::CreateEmbed,
        config: &crate::config::Config,
    ) -> &'e mut serenity::builder::CreateEmbed {
        match self {
            Message::Action {
                message,
                voice_channel,
                ..
            } => message.create_embed(embed, config, *voice_channel),
            Message::Response { message, .. } => message.create_embed(embed, config),
        }
    }*/

    pub fn create_embed(
        &self,
        config: &crate::config::Config
    ) -> CreateEmbed {
        match self {
            Message::Action {
                message,
                voice_channel,
                ..
            } => message.create_embed(config, *voice_channel),
            Message::Response { message, .. } => message.create_embed(config),
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
        thumbnail_url: Option<String>,
        time_seconds: f64,
        duration_seconds: Option<f64>,
    },
    PlayingResponse {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
        thumbnail_url: Option<String>,
        time_seconds: f64,
        duration_seconds: Option<f64>,
    },
    Played {
        song_title: String,
        song_url: String,
    },
    Finished,
    Paused {
        song_title: String,
        song_url: String,
        user_id: UserId,
    },
    Stopped {
        song_title: String,
        song_url: String,
        user_id: UserId,
    },
    NoSpeakersError,
    UnknownError,
}

/// Response messages are always sent directly as a response to a command invocation.
#[derive(Debug, Clone)]
pub enum ResponseMessage {
    Queued {
        song_title: String,
        song_url: String,
    },
    QueuedMultiple {
        count: usize,
    },
    QueuedNoSpeakers {
        song_title: String,
        song_url: String,
    },
    QueuedMultipleNoSpeakers {
        count: usize,
    },
    Replaced {
        old_song_title: String,
        old_song_url: String,
        new_song_title: String,
        new_song_url: String,
    },
    ReplaceSkipped {
        new_song_title: String,
        new_song_url: String,
        old_song_title: String,
        old_song_url: String,
        voice_channel_id: ChannelId,
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
    StopMoreVotesNeeded {
        voice_channel_id: ChannelId,
        count: usize,
    },
    NoMatchingSongsError,
    NotInVoiceChannelError,
    UnsupportedSiteError,
    SkipAlreadyVotedError {
        song_title: String,
        song_url: String,
        voice_channel_id: ChannelId,
    },
    StopAlreadyVotedError {
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
    },
}

impl ActionMessage {
    pub fn to_string(&self, config: &crate::config::Config, voice_channel_id: ChannelId) -> String {
        match self {
            ActionMessage::Playing {
                song_title,
                song_url,
                voice_channel_id,
                user_id,
                time_seconds,
                duration_seconds,
                ..
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                let user_id_string = user_id.get().to_string();
                let time_string = format_time_bar(config, *time_seconds, *duration_seconds);

                config.get_message(
                    "action.playing",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                        ("user_id", &user_id_string),
                        ("time", &time_string),
                    ],
                )
            }
            ActionMessage::PlayingResponse {
                song_title,
                song_url,
                voice_channel_id,
                time_seconds,
                duration_seconds,
                ..
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                let time_string = format_time_bar(config, *time_seconds, *duration_seconds);

                config.get_message(
                    "action.playing_response",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                        ("time", &time_string),
                    ],
                )
            }
            ActionMessage::Played {
                song_title,
                song_url,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();

                config.get_message(
                    "action.played",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                    ],
                )
            }
            ActionMessage::Finished => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "action.finished",
                    &[("voice_channel_id", &channel_id_string)],
                )
            }
            ActionMessage::Paused {
                song_title,
                song_url,
                user_id,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                let user_id_string = user_id.get().to_string();
                config.get_message(
                    "response.paused",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                        ("user_id", &user_id_string),
                    ],
                )
            }
            ActionMessage::Stopped {
                song_title,
                song_url,
                user_id,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                let user_id_string = user_id.get().to_string();
                config.get_message(
                    "response.stopped",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                        ("user_id", &user_id_string),
                    ],
                )
            }
            ActionMessage::NoSpeakersError => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "action.no_speakers_error",
                    &[("voice_channel_id", &channel_id_string)],
                )
            }
            ActionMessage::UnknownError => {
                config.get_raw_message("action.unknown_error").to_string()
            }
        }
    }

    pub fn get_thumbnail(&self) -> Option<&str> {
        match self {
            ActionMessage::Playing {
                thumbnail_url: Some(thumbnail),
                ..
            }
            | ActionMessage::PlayingResponse {
                thumbnail_url: Some(thumbnail),
                ..
            } => Some(thumbnail),
            _ => None,
        }
    }

    pub fn is_error(&self) -> bool {
        match self {
            ActionMessage::Playing { .. }
            | ActionMessage::PlayingResponse { .. }
            | ActionMessage::Played { .. }
            | ActionMessage::Finished { .. }
            | ActionMessage::Paused { .. }
            | ActionMessage::Stopped { .. } => false,
            ActionMessage::NoSpeakersError { .. } | ActionMessage::UnknownError => true,
        }
    }

    /*pub fn create_embed<'e>(
        &self,
        embed: &'e mut serenity::builder::CreateEmbed,
        config: &crate::config::Config,
        voice_channel_id: ChannelId,
    ) -> &'e mut serenity::builder::CreateEmbed {
        embed
            .description(self.to_string(config, voice_channel_id))
            .color(if self.is_error() {
                config.error_embed_color
            } else {
                config.action_embed_color
            });

        if let Some(thumbnail) = self.get_thumbnail() {
            embed.thumbnail(thumbnail);
        }

        embed
    }*/

    pub fn create_embed(&self, config: &crate::config::Config, voice_channel_id: ChannelId) -> CreateEmbed {
        todo!()
    }
}

impl ResponseMessage {
    pub fn to_string(&self, config: &crate::config::Config) -> String {
        match self {
            ResponseMessage::Queued {
                song_title,
                song_url,
            } => config.get_message(
                "response.queued",
                &[("song_title", song_title), ("song_url", song_url)],
            ),
            ResponseMessage::QueuedMultiple { count } => {
                let count_string = count.to_string();
                config.get_message("response.queued_multiple", &[("count", &count_string)])
            }
            ResponseMessage::QueuedNoSpeakers {
                song_title,
                song_url,
            } => config.get_message(
                "response.queued_no_speakers",
                &[("song_title", song_title), ("song_url", song_url)],
            ),
            ResponseMessage::QueuedMultipleNoSpeakers { count } => {
                let count_string = count.to_string();
                config.get_message(
                    "response.queued_multiple_no_speakers",
                    &[("count", &count_string)],
                )
            }
            ResponseMessage::Replaced {
                old_song_title,
                old_song_url,
                new_song_title,
                new_song_url,
            } => config.get_message(
                "response.replaced",
                &[
                    ("old_song_title", old_song_title),
                    ("old_song_url", old_song_url),
                    ("new_song_title", new_song_title),
                    ("new_song_url", new_song_url),
                ],
            ),
            ResponseMessage::ReplaceSkipped {
                new_song_title,
                new_song_url,
                old_song_title,
                old_song_url,
                voice_channel_id,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "response.replace_skipped",
                    &[
                        ("new_song_title", new_song_title),
                        ("new_song_url", new_song_url),
                        ("old_song_title", old_song_title),
                        ("old_song_url", old_song_url),
                        ("voice_channel_id", &channel_id_string),
                    ],
                )
            }
            ResponseMessage::Skipped {
                song_title,
                song_url,
                voice_channel_id,
                user_id,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                let user_id_string = user_id.get().to_string();
                config.get_message(
                    "response.skipped",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                        ("user_id", &user_id_string),
                    ],
                )
            }
            ResponseMessage::SkipMoreVotesNeeded {
                song_title,
                song_url,
                voice_channel_id,
                count,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                if *count == 1 {
                    config.get_message(
                        "response.skip_more_votes_needed.singular",
                        &[
                            ("song_title", song_title),
                            ("song_url", song_url),
                            ("voice_channel_id", &channel_id_string),
                        ],
                    )
                } else {
                    let count_string = count.to_string();
                    config.get_message(
                        "response.skip_more_votes_needed.plural",
                        &[
                            ("song_title", song_title),
                            ("song_url", song_url),
                            ("voice_channel_id", &channel_id_string),
                            ("count", &count_string),
                        ],
                    )
                }
            }
            ResponseMessage::StopMoreVotesNeeded {
                voice_channel_id,
                count,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                if *count == 1 {
                    config.get_message(
                        "response.stop_more_votes_needed.singular",
                        &[("voice_channel_id", &channel_id_string)],
                    )
                } else {
                    let count_string = count.to_string();
                    config.get_message(
                        "response.stop_more_votes_needed.plural",
                        &[
                            ("voice_channel_id", &channel_id_string),
                            ("count", &count_string),
                        ],
                    )
                }
            }
            ResponseMessage::NoMatchingSongsError => config
                .get_raw_message("response.no_matching_songs_error")
                .to_string(),
            ResponseMessage::NotInVoiceChannelError => config
                .get_raw_message("response.not_in_voice_channel_error")
                .to_string(),
            ResponseMessage::UnsupportedSiteError => config
                .get_raw_message("response.unsupported_site_error")
                .to_string(),
            ResponseMessage::SkipAlreadyVotedError {
                song_title,
                song_url,
                voice_channel_id,
            } => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "response.skip_already_voted_error",
                    &[
                        ("song_title", song_title),
                        ("song_url", song_url),
                        ("voice_channel_id", &channel_id_string),
                    ],
                )
            }
            ResponseMessage::StopAlreadyVotedError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "response.stop_already_voted_error",
                    &[("voice_channel_id", &channel_id_string)],
                )
            }
            ResponseMessage::NothingIsQueuedError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "response.nothing_is_queued_error",
                    &[("voice_channel_id", &channel_id_string)],
                )
            }
            ResponseMessage::NothingIsPlayingError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "response.nothing_is_playing_error",
                    &[("voice_channel_id", &channel_id_string)],
                )
            }
            ResponseMessage::AlreadyPlayingError { voice_channel_id } => {
                let channel_id_string = voice_channel_id.get().to_string();
                config.get_message(
                    "response.already_playing_error",
                    &[("voice_channel_id", &channel_id_string)],
                )
            }
        }
    }

    pub fn is_error(&self) -> bool {
        match self {
            ResponseMessage::Queued { .. }
            | ResponseMessage::QueuedMultiple { .. }
            | ResponseMessage::QueuedNoSpeakers { .. }
            | ResponseMessage::QueuedMultipleNoSpeakers { .. }
            | ResponseMessage::Replaced { .. }
            | ResponseMessage::ReplaceSkipped { .. }
            | ResponseMessage::Skipped { .. }
            | ResponseMessage::SkipMoreVotesNeeded { .. }
            | ResponseMessage::StopMoreVotesNeeded { .. } => false,
            ResponseMessage::NoMatchingSongsError
            | ResponseMessage::NotInVoiceChannelError
            | ResponseMessage::UnsupportedSiteError
            | ResponseMessage::SkipAlreadyVotedError { .. }
            | ResponseMessage::StopAlreadyVotedError { .. }
            | ResponseMessage::NothingIsQueuedError { .. }
            | ResponseMessage::NothingIsPlayingError { .. }
            | ResponseMessage::AlreadyPlayingError { .. } => true,
        }
    }

    pub fn create_embed<'e>(
        &self,
        config: &crate::config::Config,
    ) -> CreateEmbed {
        CreateEmbed::new()
            .color(if self.is_error() {
                config.error_embed_color
            } else {
                config.response_embed_color
            })
            .description(self.to_string(config))
    }
}
