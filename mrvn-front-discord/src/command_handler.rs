use serenity::{prelude::*, model::prelude::*};
use futures::prelude::*;
use mrvn_model::app_model::AppModel;
use mrvn_back_ytdl::song::Song;
use crate::model_delegate::ModelDelegate;
use futures::TryFutureExt;

fn playing_message(song_title: &str, song_url: &str) -> String {
    format!(":robot: :loud_sound: Playing [{}](<{}>)", song_title, song_url)
}

fn queued_message(song_title: &str, song_url: &str) -> String {
    format!(":robot: :see_no_evil: Queued [{}](<{}>)", song_title, song_url)
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

fn unknown_error_message() -> &'static str {
    ":robot: :weary: An error occurred."
}

pub struct CommandHandler {
    pub model: AppModel<Song>,
}

impl CommandHandler {
    async fn handle_command(&self, ctx: Context, command: &interactions::application_command::ApplicationCommandInteraction) -> Result<(), crate::error::Error> {
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
                            response.content(no_matching_songs_message())
                        }).await.map_err(crate::error::Error::Serenity)?;
                        return Ok(())
                    },
                    Err(err) => return Err(crate::error::Error::Backend(err)),
                };

                let song_title = song.title().to_string();
                let song_url = song.url().to_string();
                log::trace!("Resolved song \"{}\" (\"{}\")", song_title, song_url);

                let entry_to_play = self.model.get(guild_id, |guild_model| {
                    guild_model.push_entry(user_id, song);
                    delegate.get_user_voice_channel(user_id).and_then(|channel_id| guild_model.next_channel_entry(&delegate, channel_id))
                }).await;

                // We could be in one of three states:
                // - The song we just queued is now playing. Reply with a "Playing ..." message.
                // - Something was already playing so the song isn't playing. Reply with a
                //   "Queued ..." message.
                // - The song is queued but for whatever reason this has started a different song
                //   playing. Reply with a "Queued ..." message and immediately send a "Playing ..."
                //   message.
                match entry_to_play {
                    // The song we just queued is now playing
                    Some(play_song) if play_song.url() == song_url => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.content(playing_message(&song_title, &song_url))
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }

                    // Song was queued, started playing a different song
                    Some(play_song) => {
                        let started_title = play_song.title();
                        let started_url = play_song.url();

                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.content(queued_message(&song_title, &song_url))
                        }).await.map_err(crate::error::Error::Serenity)?;
                        command.channel_id.send_message(&ctx.http, |message| {
                            message.content(playing_message(started_title, started_url))
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }

                    // Song is queued, nothing new is playing
                    None => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.content(queued_message(&song_title, &song_url))
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                }

                // todo: actually play the song

                Ok(())
            }
            "replace" => Ok(()),
            "pause" => Ok(()),
            "unpause" => Ok(()),
            "join" => {
                let delegate = ModelDelegate::new(&ctx, guild_id).await?;

                let channel_id = match delegate.get_user_voice_channel(user_id) {
                    Some(channel) => channel,
                    None => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.content(not_in_vc_message())
                        }).await.map_err(crate::error::Error::Serenity)?;
                        return Ok(());
                    },
                };

                let entry_to_play = self.model.get(guild_id, |guild_model| {
                    guild_model.next_channel_entry(&delegate, channel_id)
                }).await;

                match entry_to_play {
                    Some(play_song) => {
                        let started_title = play_song.title();
                        let started_url = play_song.url();

                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.content(playing_message(started_title, started_url))
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                    None => {
                        command.edit_original_interaction_response(&ctx.http, |response| {
                            response.content(nothing_queued_message())
                        }).await.map_err(crate::error::Error::Serenity)?;
                    }
                }

                // todo: actually play the song

                Ok(())
            }
            command_name => Err(crate::error::Error::UnknownCommand(command_name.to_string()).into()),
        }
    }
}

#[serenity::async_trait]
impl EventHandler for CommandHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        log::info!("Command client is connected as {}", ready.user.name);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            if let Err(why) = command.create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
            }).await {
                log::error!("Error while sending deferred message: {}", why);
            }

            if let Err(why) = self.handle_command(ctx.clone(), &command).await {
                log::error!("Error while handling command: {}", why);
                let edit_res = command.edit_original_interaction_response(&ctx.http, |response| {
                    response.content(unknown_error_message())
                }).await;
                if let Err(why) = edit_res {
                    log::error!("Error while sending error response: {}", why);
                }
            }
        }
    }
}
