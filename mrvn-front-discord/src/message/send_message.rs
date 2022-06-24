use crate::config::Config;
use crate::message::default_action_delegate::DefaultActionDelegate;
use crate::message::{ActionUpdater, Message};
use futures::prelude::*;
use mrvn_back_ytdl::Song;
use mrvn_model::{ChannelActionMessage, GuildModel};
use serenity::model::prelude::ChannelId;
use serenity::{
    client::Context,
    model::interactions::{
        application_command::ApplicationCommandInteraction, InteractionResponseType,
    },
};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub enum SendMessageDestination<'interaction> {
    Channel(ChannelId),
    Interaction {
        interaction: &'interaction ApplicationCommandInteraction,
        is_edit: bool,
    },
}

pub async fn send_messages(
    config: &Arc<Config>,
    ctx: &Context,
    destination: SendMessageDestination<'_>,
    guild_model: &mut GuildModel<Song>,
    mut messages: Vec<Message>,
) -> Result<(), crate::error::Error> {
    let message_channel_id = match destination {
        SendMessageDestination::Channel(channel) => channel,
        SendMessageDestination::Interaction { interaction, .. } => interaction.channel_id,
    };

    // Action messages are special: we only keep the latest one around. This also means out of
    // this list we only want to send the last action message.
    let maybe_last_action_message =
        messages
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, message)| match message {
                Message::Action { voice_channel, .. } => Some((index, *voice_channel)),
                _ => None,
            });

    if let Some((last_action_message_index, _)) = maybe_last_action_message {
        let mut index = 0;
        messages.retain(|message| {
            let is_valid = !message.is_action() || index == last_action_message_index;
            index += 1;
            is_valid
        });
    }

    let mut messages_iter = messages.into_iter();

    // Send the first message as an interaction response, if our destination is an interaction.
    let maybe_first_message = match destination {
        SendMessageDestination::Channel(_) => None,
        SendMessageDestination::Interaction { .. } => messages_iter.next(),
    };
    let first_message_future = async {
        let message_maybe = match (destination, maybe_first_message) {
            (
                SendMessageDestination::Interaction {
                    interaction,
                    is_edit,
                },
                Some(first_message),
            ) => {
                let channel_message = if is_edit {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.create_embed(|embed| first_message.create_embed(embed, config))
                        })
                        .await
                        .map_err(crate::error::Error::Serenity)?
                } else {
                    interaction
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|data| {
                                    data.create_embed(|embed| {
                                        first_message.create_embed(embed, config)
                                    })
                                })
                        })
                        .await
                        .map_err(crate::error::Error::Serenity)?;
                    interaction
                        .get_interaction_response(&ctx.http)
                        .await
                        .map_err(crate::error::Error::Serenity)?
                };

                match first_message {
                    Message::Action {
                        delegate,
                        voice_channel,
                        ..
                    } => {
                        let delegate = delegate.unwrap_or_else(|| Box::new(DefaultActionDelegate));
                        Some(ChannelActionMessage {
                            frontend_handle: delegate.start(ActionUpdater::new(
                                channel_message.channel_id,
                                channel_message.id,
                                voice_channel,
                                true,
                                config.clone(),
                                ctx.http.clone(),
                            )),
                        })
                    }
                    Message::Response(_) => None,
                }
            }
            _ => None,
        };

        Ok(message_maybe)
    };

    // Send each remaining message as a regular message. If the message is the possible one
    // action message, keep track of its ID so we can record it later.
    let remaining_messages_future = future::try_join_all(messages_iter.map(|message| async move {
        let channel_message = message_channel_id
            .send_message(&ctx.http, |create_message| {
                create_message.embed(|embed| message.create_embed(embed, config))
            })
            .await
            .map_err(crate::error::Error::Serenity)?;

        match message {
            Message::Action {
                delegate,
                voice_channel,
                ..
            } => {
                let delegate = delegate.unwrap_or_else(|| Box::new(DefaultActionDelegate));
                Ok(Some(ChannelActionMessage {
                    frontend_handle: delegate.start(ActionUpdater::new(
                        channel_message.channel_id,
                        channel_message.id,
                        voice_channel,
                        false,
                        config.clone(),
                        ctx.http.clone(),
                    )),
                }))
            }
            Message::Response(_) => Ok(None),
        }
    }));

    // Delete the guild's latest action message from before this operation, if this operation
    // sent an action message.
    if let Some((_, last_action_message_channel)) = maybe_last_action_message {
        guild_model.clear_last_action_message(last_action_message_channel);
    }

    // Execute all the message sending!
    let (first_message, remaining_messages) =
        futures::try_join!(first_message_future, remaining_messages_future)?;

    // Set the channel's last action message to the message we sent, if there was one.
    if let Some((_, last_action_message_channel)) = maybe_last_action_message {
        let maybe_sent_message = std::iter::once(first_message)
            .chain(remaining_messages.into_iter())
            .find_map(|maybe_message| maybe_message);

        guild_model.set_last_action_message(last_action_message_channel, maybe_sent_message);
    }

    Ok(())
}
