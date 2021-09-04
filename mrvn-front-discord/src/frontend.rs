use mrvn_back_ytdl::{Brain, Song, EndedHandler};
use mrvn_model::{AppModel, GuildModel, NextEntry, SkipStatus};
use std::sync::Arc;
use serenity::{prelude::*, model::prelude::{UserId, GuildId, interactions, application_command}};
use crate::config::Config;
use std::ops::DerefMut;
use crate::message::{send_messages, Message, ResponseMessage, ActionMessage, SendMessageDestination};
use crate::model_delegate::ModelDelegate;
use serenity::model::id::ChannelId;

pub struct Frontend {
    config: Arc<Config>,
    backend_brain: Brain,
    model: AppModel<Song>,
}

impl Frontend {
    pub fn new(
        config: Arc<Config>,
        backend_brain: Brain,
        model: AppModel<Song>,
    ) -> Frontend {
        Frontend {
            config,
            backend_brain,
            model,
        }
    }

    pub async fn handle_command(
        self: &Arc<Self>,
        ctx: &Context,
        command: &interactions::application_command::ApplicationCommandInteraction
    ) -> Result<(), crate::error::Error> {
        let guild_id = command.guild_id.ok_or(crate::error::Error::NoGuild)?;
        let message_channel_id = command.channel_id;

        // Ensure we have the guild locked for the duration of the command.
        let guild_model_handle = self.model.get(guild_id);
        let mut guild_model = guild_model_handle.lock().await;
        guild_model.set_message_channel(Some(message_channel_id));

        // Execute the command
        let messages = self.handle_guild_command(ctx, command, guild_id, guild_model.deref_mut()).await?;

        // Send all messages
        send_messages(&self.config, ctx, SendMessageDestination::Interaction(command), guild_model.deref_mut(), messages).await?;

        Ok(())
    }

    async fn handle_guild_command(
        self: &Arc<Self>,
        ctx: &Context,
        command: &interactions::application_command::ApplicationCommandInteraction,
        guild_id: GuildId,
        guild_model: &mut GuildModel<Song>,
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        let user_id = command.user.id;
        match command.data.name.as_str() {
            "play" => {
                let maybe_term = match command.data.options.get(0).and_then(|val| val.resolved.as_ref()) {
                    Some(application_command::ApplicationCommandInteractionDataOptionValue::String(val)) => Some(val.clone()),
                    _ => None,
                };

                match maybe_term {
                    Some(term) => {
                        log::debug!("Received play, interpreted as queue-play \"{}\"", term);
                        self.handle_queue_play_command(ctx, user_id, guild_id, guild_model, term).await
                    }
                    None => {
                        log::debug!("Received play, interpreted as unpause");
                        self.handle_unpause_command(ctx, user_id, guild_id, guild_model).await
                    }
                }
            }
            "replace" => {
                let term = match command.data.options.get(0).and_then(|val| val.resolved.as_ref()) {
                    Some(application_command::ApplicationCommandInteractionDataOptionValue::String(val)) => val.clone(),
                    _ => "".to_string(),
                };

                log::debug!("Received replace \"{}\"", term);
                self.handle_replace_command(ctx, user_id, guild_id, guild_model, term).await
            }
            "pause" => {
                log::debug!("Received pause");
                self.handle_pause_command(ctx, user_id, guild_id).await
            }
            "skip" => {
                log::debug!("Received skip");
                self.handle_skip_command(ctx, user_id, guild_id, guild_model).await
            }
            command_name => Err(crate::error::Error::UnknownCommand(command_name.to_string())),
        }
    }


    async fn handle_queue_play_command(
        self: &Arc<Self>,
        ctx: &Context,
        user_id: UserId,
        guild_id: GuildId,
        guild_model: &mut GuildModel<Song>,
        term: String,
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        let delegate_future = ModelDelegate::new(ctx, guild_id);
        let song_future = async {
            Song::load(term, user_id).await.map_err(crate::error::Error::Backend)
        };

        let (delegate, song) = match futures::try_join!(delegate_future, song_future) {
            Ok((delegate, song)) => (delegate, song),
            Err(crate::error::Error::Backend(mrvn_back_ytdl::Error::NoSongsFound)) => {
                return Ok(vec![Message::Response(ResponseMessage::NoMatchingSongsError)]);
            },
            Err(err) => return Err(err),
        };

        let song_metadata = song.metadata.clone();
        log::trace!("Resolved song query as {} (\"{}\")", song_metadata.url, song_metadata.title);

        guild_model.push_entry(user_id, song);

        // From this point on the user needs to be in a channel, otherwise the song will only stay
        // queued.
        let channel_id = match delegate.get_user_voice_channel(user_id) {
            Some(channel) => channel,
            None => {
                log::trace!("User is not in any voice channel, song will remain queued");
                return Ok(vec![Message::Response(ResponseMessage::Queued {
                    song_title: song_metadata.title,
                    song_url: song_metadata.url,
                })])
            },
        };

        // Find a speaker that will be able to play in this channel. We do this before checking if
        // we actually need to play anything so the song can stay in the queue if a speaker isn't
        // found.
        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        let guild_speaker = match guild_speakers_ref.find_to_play_in_channel(channel_id) {
            Some(speaker) => speaker,
            None => {
                log::trace!("No speakers are available to handle playback, song will remain queued");
                return Ok(vec![Message::Response(ResponseMessage::QueuedNoSpeakers {
                    song_title: song_metadata.title,
                    song_url: song_metadata.url,
                })])
            }
        };

        // Play a song if the model indicates one isn't playing.
        let next_song = match guild_model.next_channel_entry(&delegate, channel_id) {
            NextEntry::Entry(song) => song,
            NextEntry::AlreadyPlaying | NextEntry::NoneAvailable => {
                log::trace!("Channel is already playing, song will remain queued");
                return Ok(vec![Message::Response(ResponseMessage::Queued {
                    song_title: song_metadata.title,
                    song_url: song_metadata.url,
                })])
            }
        };

        let next_metadata = next_song.metadata.clone();
        log::trace!("Playing \"{}\" to speaker", next_metadata.title);
        guild_speaker.play(channel_id, next_song, EndedDelegate {
            frontend: self.clone(),
            ctx: ctx.clone(),
            guild_id,
            channel_id,
        }).await.map_err(crate::error::Error::Backend)?;

        // We could be in one of two states:
        //  - The song that's now playing is the one we just queued, in which case we only show a
        //    "playing" message.
        //  - We queued a song and started a different song, which can happen if there were other
        //    songs waiting but we weren't playing at the time. In this case we show a "queued"
        //    message and a "playing" message.
        if next_metadata.url == song_metadata.url {
            Ok(vec![Message::Action(ActionMessage::Playing {
                song_title: song_metadata.title,
                song_url: song_metadata.url,
                voice_channel_id: channel_id,
                user_id,
            })])
        } else {
            Ok(vec![
                Message::Response(ResponseMessage::Queued {
                    song_title: song_metadata.title,
                    song_url: song_metadata.url,
                }),
                Message::Action(ActionMessage::Playing {
                    song_title: next_metadata.title,
                    song_url: next_metadata.url,
                    voice_channel_id: channel_id,
                    user_id: next_metadata.user_id,
                })
            ])
        }
    }

    async fn handle_unpause_command(
        self: &Arc<Self>,
        ctx: &Context,
        user_id: UserId,
        guild_id: GuildId,
        guild_model: &mut GuildModel<Song>,
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        let delegate = ModelDelegate::new(ctx, guild_id).await?;
        let channel_id = match delegate.get_user_voice_channel(user_id) {
            Some(channel) => channel,
            None => return Ok(vec![Message::Response(ResponseMessage::NotInVoiceChannelError)])
        };

        // See if there's currently a speaker in this channel to unpause.
        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        if let Some((guild_speaker, active_metadata)) = guild_speakers_ref.find_active_in_channel(channel_id) {
            return if guild_speaker.is_paused() {
                log::trace!("Found a paused speaker in the user's voice channel, starting playback");
                guild_speaker.unpause().map_err(crate::error::Error::Backend)?;
                Ok(vec![Message::Action(ActionMessage::Playing {
                    song_title: active_metadata.title.clone(),
                    song_url: active_metadata.url.clone(),
                    voice_channel_id: channel_id,
                    user_id: active_metadata.user_id,
                })])
            } else {
                log::trace!("Found an unpaused speaker in the user's voice channel, playback will continue");
                Ok(vec![Message::Response(ResponseMessage::AlreadyPlayingError {
                    voice_channel_id: channel_id,
                })])
            };
        };

        // Otherwise, try starting to play in this channel.
        let guild_speaker = match guild_speakers_ref.find_to_play_in_channel(channel_id) {
            Some(speaker) => speaker,
            None => {
                log::trace!("No speakers are available to handle playback, nothing will be played");
                return Ok(vec![Message::Action(ActionMessage::NoSpeakersError {
                    voice_channel_id: channel_id,
                })])
            },
        };
        let next_song = match guild_model.next_channel_entry(&delegate, channel_id) {
            NextEntry::Entry(song) => song,
            NextEntry::AlreadyPlaying | NextEntry::NoneAvailable => {
                log::trace!("No songs are available to play back in the channel, nothing will be played");
                return Ok(vec![Message::Response(ResponseMessage::NothingIsQueuedError {
                    voice_channel_id: channel_id,
                })])
            }
        };

        let next_metadata = next_song.metadata.clone();
        log::trace!("Playing \"{}\" to speaker", next_metadata.title);
        guild_speaker.play(channel_id, next_song, EndedDelegate {
            frontend: self.clone(),
            ctx: ctx.clone(),
            guild_id,
            channel_id,
        }).await.map_err(crate::error::Error::Backend)?;

        Ok(vec![Message::Action(ActionMessage::Playing {
            song_title: next_metadata.title,
            song_url: next_metadata.url,
            voice_channel_id: channel_id,
            user_id: next_metadata.user_id,
        })])
    }

    async fn handle_replace_command(
        self: &Arc<Self>,
        _ctx: &Context,
        _user_id: UserId,
        _guild_id: GuildId,
        _guild_model: &mut GuildModel<Song>,
        _term: String,
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        // todo
        Ok(vec![])
    }

    async fn handle_pause_command(
        self: &Arc<Self>,
        ctx: &Context,
        user_id: UserId,
        guild_id: GuildId,
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        let delegate = ModelDelegate::new(ctx, guild_id).await?;
        let channel_id = match delegate.get_user_voice_channel(user_id) {
            Some(channel) => channel,
            None => return Ok(vec![Message::Response(ResponseMessage::NotInVoiceChannelError)])
        };

        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        match guild_speakers_ref.find_active_in_channel(channel_id) {
            Some((guild_speaker, active_metadata)) => {
                if guild_speaker.is_paused() {
                    log::trace!("Found a paused speaker in the user's voice channel, playback will remain paused");
                    Ok(vec![Message::Response(ResponseMessage::NothingIsPlayingError {
                        voice_channel_id: channel_id,
                    })])
                } else {
                    log::trace!("Found an unpaused speaker in the user's voice channel, playback will be paused");
                    guild_speaker.pause().map_err(crate::error::Error::Backend)?;
                    Ok(vec![Message::Response(ResponseMessage::Paused {
                        song_title: active_metadata.title.clone(),
                        song_url: active_metadata.url.clone(),
                        voice_channel_id: channel_id,
                        user_id: active_metadata.user_id,
                    })])
                }
            },
            _ => {
                log::trace!("No speakers are in the user's voice channel, playback will not change");
                Ok(vec![Message::Response(ResponseMessage::NothingIsPlayingError {
                    voice_channel_id: channel_id,
                })])
            }
        }
    }

    async fn handle_skip_command(
        self: &Arc<Self>,
        ctx: &Context,
        user_id: UserId,
        guild_id: GuildId,
        guild_model: &mut GuildModel<Song>,
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        let delegate = ModelDelegate::new(&ctx, guild_id).await?;
        let channel_id = match delegate.get_user_voice_channel(user_id) {
            Some(channel) => channel,
            None => return Ok(vec![Message::Response(ResponseMessage::NotInVoiceChannelError)])
        };

        let skip_status = guild_model.vote_for_skip(&delegate, channel_id, user_id);

        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        let maybe_guild_speaker = guild_speakers_ref.find_active_in_channel(channel_id);

        match (skip_status, maybe_guild_speaker) {
            (SkipStatus::OkToSkip, Some((guild_speaker, active_metadata))) => {
                log::trace!("Skip command passed preconditions, stopping current playback");
                guild_speaker.stop().map_err(crate::error::Error::Backend)?;
                Ok(vec![Message::Response(ResponseMessage::Skipped {
                    song_title: active_metadata.title.clone(),
                    song_url: active_metadata.url.clone(),
                    voice_channel_id: channel_id,
                    user_id: active_metadata.user_id,
                })])
            }
            (SkipStatus::AlreadyVoted, Some((_, active_metadata))) => {
                log::trace!("User attempting to skip has already voted, not stopping playback");
                Ok(vec![Message::Response(ResponseMessage::SkipAlreadyVotedError {
                    song_title: active_metadata.title.clone(),
                    song_url: active_metadata.url.clone(),
                    voice_channel_id: channel_id,
                })])
            }
            (SkipStatus::NeedsMoreVotes(count), Some((_, active_metadata))) => {
                log::trace!("Skip vote has been counted but more are needed, not stopping playback");
                Ok(vec![Message::Response(ResponseMessage::SkipMoreVotesNeeded {
                    song_title: active_metadata.title.clone(),
                    song_url: active_metadata.url.clone(),
                    voice_channel_id: channel_id,
                    count,
                })])
            }
            (SkipStatus::NothingPlaying, _) => {
                log::trace!("Nothing is playing in the user's voice channel, not stopping playback");
                Ok(vec![Message::Response(ResponseMessage::NothingIsPlayingError {
                    voice_channel_id: channel_id,
                })])
            }
            (_, None) => {
                log::warn!("Out of sync: model says song is playing, but the speaker disagrees");
                Ok(vec![Message::Response(ResponseMessage::NothingIsPlayingError {
                    voice_channel_id: channel_id,
                })])
            }
        }
    }

    async fn handle_playback_ended(self: Arc<Self>, ctx: Context, guild_id: GuildId, channel_id: ChannelId) {
        log::trace!("Playback has ended, preparing to play the next available song");

        let guild_model_handle = self.model.get(guild_id);
        let mut guild_model = guild_model_handle.lock().await;

        let maybe_message_channel = guild_model.message_channel();
        let messages = self.continue_channel_playback(&ctx, guild_id, guild_model.deref_mut(), channel_id).await;
        let send_result = match (messages, maybe_message_channel) {
            (Ok(messages), Some(message_channel)) => {
                send_messages(&self.config, &ctx, SendMessageDestination::Channel(message_channel), guild_model.deref_mut(), messages).await
            },
            (Err(why), Some(message_channel)) => {
                log::error!("Error while continuing playback: {}", why);
                send_messages(&self.config, &ctx, SendMessageDestination::Channel(message_channel), guild_model.deref_mut(), vec![
                    Message::Action(ActionMessage::UnknownError)
                ]).await
            },
            (Err(why), _) => Err(why),
            (_, None) => Ok(()),
        };

        if let Err(why) = send_result {
            log::error!("Error while continuing playback: {}", why);
        }
    }

    async fn continue_channel_playback(
        self: &Arc<Self>,
        ctx: &Context,
        guild_id: GuildId,
        guild_model: &mut GuildModel<Song>,
        channel_id: ChannelId
    ) -> Result<Vec<crate::message::Message>, crate::error::Error> {
        let delegate = ModelDelegate::new(&ctx, guild_id).await?;

        let guild_speakers_handle = self.backend_brain.guild_speakers(guild_id);
        let mut guild_speakers_ref = guild_speakers_handle.lock().await;
        let guild_speaker = match guild_speakers_ref.find_to_play_in_channel(channel_id) {
            Some(speaker) => speaker,
            None => {
                log::trace!("No speakers are available to handle playback, nothing will be played");
                return Ok(vec![Message::Action(ActionMessage::NoSpeakersError {
                    voice_channel_id: channel_id,
                })])
            }
        };

        let next_song = match guild_model.next_channel_entry_finished(&delegate, channel_id) {
            Some(song) => song,
            None => {
                log::trace!("No songs are available to play in the channel, nothing will be played");
                return Ok(vec![Message::Action(ActionMessage::Finished {
                    voice_channel_id: channel_id,
                })])
            }
        };

        let next_metadata = next_song.metadata.clone();
        log::trace!("Playing \"{}\" to speaker", next_metadata.title);
        guild_speaker.play(channel_id, next_song, EndedDelegate {
            frontend: self.clone(),
            ctx: ctx.clone(),
            guild_id,
            channel_id,
        }).await.map_err(crate::error::Error::Backend)?;

        Ok(vec![Message::Action(ActionMessage::Playing {
            song_title: next_metadata.title,
            song_url: next_metadata.url,
            voice_channel_id: channel_id,
            user_id: next_metadata.user_id,
        })])
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
        tokio::task::spawn(self.frontend.handle_playback_ended(self.ctx, self.guild_id, self.channel_id));
    }
}
