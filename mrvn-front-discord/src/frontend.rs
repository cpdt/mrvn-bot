use serenity::{prelude::*, model::prelude::*};
use mrvn_back_ytdl::brain::Brain;
use mrvn_model::app_model::AppModel;
use mrvn_back_ytdl::song::Song;
use crate::model_delegate::ModelDelegate;
use std::ops::DerefMut;
use mrvn_model::guild_model::GuildModel;
use std::sync::Arc;
use mrvn_back_ytdl::speaker::EndedHandler;

fn playing_message(song_title: &str, song_url: &str, channel_id: ChannelId) -> String {
    format!(":robot: :loud_sound: Playing [{}](<{}>) in <#{}>", song_title, song_url, channel_id.0)
}

fn queued_message(song_title: &str, song_url: &str) -> String {
    format!(":robot: :see_no_evil: Queued [{}](<{}>)", song_title, song_url)
}

fn queued_no_speakers_message(song_title: &str, song_url: &str) -> String {
    format!(":robot: :see_no_evil: Queued [{}](<{}>). No bots are available right now, use `/join` when one is to start playing here.", song_title, song_url)
}

fn finished_queue_message(channel_id: ChannelId) -> String {
    format!(":robot: :blush: Nothing left to play in <#{}>", channel_id.0)
}

fn no_matching_songs_message() -> &'static str {
    ":robot: :flushed: No matching songs were found."
}

fn not_in_vc_message() -> &'static str {
    ":robot: :weary: You're not in a voice channel."
}

fn nothing_queued_message() -> &'static str {
    ":robot: :weary: Nothing is queued to play in this channel."
}

fn no_speakers_message() -> &'static str {
    ":robot: :weary: No bots are available right now."
}

fn unknown_error_message() -> &'static str {
    ":robot: :weary: An error occurred."
}

enum QueueResult {
    Queued,
    QueuedNoSpeakers,
    Playing {
        title: String,
        url: String,
        channel_id: ChannelId,
    }
}

enum PlayResult {
    NothingToPlay,
    NoSpeakers,
    Playing {
        title: String,
        url: String,
    }
}

struct EndedDelegate {
    frontend: Arc<Frontend>,
    ctx: Context,
    guild_id: GuildId,
    channel_id: ChannelId,
}

impl EndedHandler for EndedDelegate {
    fn on_ended(self) {
        tokio::task::spawn(self.frontend.continue_channel_playback(self.ctx, self.guild_id, self.channel_id));
    }
}

pub struct Frontend {
    backend_brain: Brain,
    model: AppModel<Song>,
}

impl Frontend {
    pub fn new() -> Frontend {
        Frontend {
            backend_brain: Brain::new(),
            model: AppModel::new(),
        }
    }

    pub fn backend_mut(&mut self) -> &mut Brain {
        &mut self.backend_brain
    }

    async fn queue_and_play(self: Arc<Self>, ctx: Context, delegate: &ModelDelegate, guild_id: GuildId, user_id: UserId, message_channel: ChannelId, song: Song) -> Result<QueueResult, crate::error::Error> {
        let guild_model_handle = self.model.get(guild_id);
        let mut guild_model = guild_model_handle.lock().await;

        guild_model.set_message_channel(Some(message_channel));
        guild_model.push_entry(user_id, song);

        // From this point on the user needs to be a channel, otherwise the song stays queued.
        let channel_id = match delegate.get_user_voice_channel(user_id) {
            Some(channel) => channel,
            None => return Ok(QueueResult::Queued),
        };

        Ok(match self.play_next_in_channel(ctx, guild_model.deref_mut(), delegate, guild_id, channel_id).await? {
            PlayResult::NothingToPlay => QueueResult::Queued,
            PlayResult::NoSpeakers => QueueResult::QueuedNoSpeakers,
            PlayResult::Playing { title, url } => QueueResult::Playing { title, url, channel_id },
        })
    }

    async fn play_next_in_channel(self: Arc<Self>, ctx: Context, guild_model: &mut GuildModel<Song>, delegate: &ModelDelegate, guild_id: GuildId, channel_id: ChannelId) -> Result<PlayResult, crate::error::Error> {
        // Find a speaker we can use to play in this channel, if one is available.
        // We do this here so we avoid touching the queue until we're certain we can play.
        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        let guild_speaker = match guild_speakers_ref.for_channel(channel_id) {
            Some(speaker) => speaker,
            None => return Ok(PlayResult::NoSpeakers),
        };

        let maybe_next_song = guild_model.next_channel_entry(delegate, channel_id);
        let next_song = match maybe_next_song {
            Some(song) => song,
            None => return Ok(PlayResult::NothingToPlay),
        };

        let next_title = next_song.title().to_string();
        let next_url = next_song.url().to_string();

        guild_speaker.play(channel_id, next_song, EndedDelegate {
            frontend: self.clone(),
            ctx,
            guild_id,
            channel_id,
        }).await.map_err(crate::error::Error::Backend)?;

        Ok(PlayResult::Playing {
            title: next_title,
            url: next_url,
        })
    }

    async fn continue_channel_playback(self: Arc<Self>, ctx: Context, guild_id: GuildId, channel_id: ChannelId) {
        let play_result = {
            let guild_model_handle = self.model.get(guild_id);
            let mut guild_model = guild_model_handle.lock().await;
            self.clone().continue_channel_playback_inner(ctx.clone(), &mut guild_model, guild_id, channel_id).await
        };

        let message_channel = {
            let guild_model_handle = self.model.get(guild_id);
            let mut guild_model = guild_model_handle.lock().await;
            guild_model.message_channel()
        };

        let message_result = match (play_result, message_channel) {
            (Ok(PlayResult::NothingToPlay), Some(channel)) => channel.send_message(&ctx.http, |message| {
                    message.embed(|embed| {
                        embed.description(finished_queue_message(channel_id))
                    })
                }).await.map(|_| ()),
            (Ok(PlayResult::NoSpeakers), Some(channel)) => channel.send_message(&ctx.http, |message| {
                message.embed(|embed| {
                    embed.description(no_speakers_message())
                })
            }).await.map(|_| ()),
            (Ok(PlayResult::Playing { title, url }), Some(channel)) => channel.send_message(&ctx.http, |message| {
                    message.embed(|embed| {
                        embed.description(playing_message(&title, &url, channel_id))
                    })
                }).await.map(|_| ()),
            (Err(why), Some(channel)) => {
                log::error!("Error while continuing playback: {}", why);
                channel.send_message(&ctx.http, |message| {
                    message.embed(|embed| {
                        embed.description(unknown_error_message())
                    })
                }).await.map(|_| ())
            }
            (Err(why), None) => {
                log::error!("Error while continuing playback: {}", why);
                Ok(())
            }
            (_, None) => Ok(())
        };
        if let Err(why) = message_result {
            log::error!("Error while sending update message: {}", why);
        };
    }

    async fn continue_channel_playback_inner(self: Arc<Self>, ctx: Context, guild_model: &mut GuildModel<Song>, guild_id: GuildId, channel_id: ChannelId) -> Result<PlayResult, crate::error::Error> {
        let delegate = ModelDelegate::new(&ctx, guild_id).await?;

        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        let guild_speaker = match guild_speakers_ref.for_channel(channel_id) {
            Some(speaker) => speaker,
            None => return Ok(PlayResult::NoSpeakers),
        };

        let maybe_next_song = guild_model.next_channel_entry_finished(&delegate, channel_id);
        let next_song = match maybe_next_song {
            Some(song) => song,
            None => return Ok(PlayResult::NothingToPlay),
        };

        let next_title = next_song.title().to_string();
        let next_url = next_song.url().to_string();

        guild_speaker.play(channel_id, next_song, EndedDelegate {
            frontend: self.clone(),
            ctx,
            guild_id,
            channel_id,
        }).await.map_err(crate::error::Error::Backend)?;

        Ok(PlayResult::Playing {
            title: next_title,
            url: next_url,
        })
    }

    pub async fn handle_command(self: Arc<Self>, ctx: Context, command: &interactions::application_command::ApplicationCommandInteraction) -> Result<(), crate::error::Error> {
        let user_id = command.user.id;
        let guild_id = command.guild_id.ok_or(crate::error::Error::NoGuild)?;

        match command.data.name.as_str() {
            "play" => {
                log::trace!("Received play command");

                let term = match command.data.options.get(0).and_then(|val| val.resolved.as_ref()) {
                    Some(application_command::ApplicationCommandInteractionDataOptionValue::String(val)) => val.clone(),
                    _ => return Err(crate::error::Error::MissingOption("term")),
                };

                let delegate_future = ModelDelegate::new(&ctx, guild_id);
                let song_future = Song::load(term);
                let (delegate_res, song_res) = futures::join!(delegate_future, song_future);

                let delegate = delegate_res?;
                let song = match song_res {
                    Ok(song) => song,
                    Err(mrvn_back_ytdl::error::Error::NoSongsFound) => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(no_matching_songs_message())
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                        return Ok(())
                    },
                    Err(err) => return Err(crate::error::Error::Backend(err)),
                };

                let song_title = song.title().to_string();
                let song_url = song.url().to_string();
                log::trace!("Resolved song \"{}\" (\"{}\")", song_title, song_url);

                let queue_result = self.queue_and_play(ctx.clone(), &delegate, guild_id, user_id, command.channel_id, song).await?;
                match queue_result {
                    QueueResult::Queued => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(queued_message(&song_title, &song_url))
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                    QueueResult::QueuedNoSpeakers => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(queued_no_speakers_message(&song_title, &song_url))
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                    QueueResult::Playing { url, channel_id, .. } if url == song_url => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(playing_message(&song_title, &song_url, channel_id))
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                    QueueResult::Playing { title, url, channel_id } => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(queued_message(&song_title, &song_url))
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                        command.channel_id.send_message(&ctx.http, |message| {
                            message.embed(|embed| {
                                embed.description(playing_message(&title, &url, channel_id))
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                }

                Ok(())
            }
            "replace" => {
                log::trace!("Received replace command");
                Ok(())
            },
            "pause" => {
                log::trace!("Received pause command");
                Ok(())
            },
            "unpause" => {
                log::trace!("Received unpause command");
                Ok(())
            },
            "join" => {
                log::trace!("Received join command");

                let delegate = ModelDelegate::new(&ctx, guild_id).await?;

                let channel_id = match delegate.get_user_voice_channel(user_id) {
                    Some(channel) => channel,
                    None => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(not_in_vc_message())
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                        return Ok(());
                    },
                };

                let guild_model_handle = self.model.get(guild_id);
                let mut guild_model = guild_model_handle.lock().await;
                match self.play_next_in_channel(ctx.clone(), guild_model.deref_mut(), &delegate, guild_id, channel_id).await? {
                    PlayResult::NothingToPlay => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(nothing_queued_message())
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                    PlayResult::NoSpeakers => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(no_speakers_message())
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                    PlayResult::Playing { title, url } => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| {
                                embed.description(playing_message(&title, &url, channel_id))
                            })
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                };
                Ok(())
            }
            command_name => Err(crate::error::Error::UnknownCommand(command_name.to_string()).into()),
        }
    }
}
