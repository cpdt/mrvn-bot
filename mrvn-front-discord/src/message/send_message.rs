use serenity::{
    client::Context,
    model::interactions::{application_command::ApplicationCommandInteraction, InteractionResponseType}
};
use mrvn_model::{GuildModel, GuildActionMessage};
use mrvn_back_ytdl::Song;
use futures::prelude::*;
use crate::message::Message;
use crate::config::Config;
use serenity::model::prelude::ChannelId;

#[derive(Clone, Copy)]
pub enum SendMessageDestination<'interaction> {
    Channel(ChannelId),
    Interaction {
        interaction: &'interaction ApplicationCommandInteraction,
        is_edit: bool,
    }
}

pub async fn send_messages(
    config: &Config,
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
    let maybe_last_action_message_index = messages.iter().rposition(|message| message.is_action());
    if let Some(last_action_message_index) = maybe_last_action_message_index {
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
        if let (SendMessageDestination::Interaction { interaction, is_edit }, Some(first_message)) = (destination, maybe_first_message) {
            if is_edit {
                interaction.edit_original_interaction_response(&ctx.http, |response| {
                    response.create_embed(|embed| {
                        embed.description(first_message.to_string(config))
                    })
                }).await.map_err(crate::error::Error::Serenity)?;
            } else {
                interaction.create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|data| {
                            data.create_embed(|embed| embed.description(first_message.to_string(config)))
                        })
                }).await.map_err(crate::error::Error::Serenity)?;
            }
        }
        Ok(())
    };

    // Send each remaining message as a regular message. If the message is the possible one
    // action message, keep track of its ID so we can record it later.
    let remaining_messages_future = future::try_join_all(messages_iter.map(|message| async move {
        let channel_message = message_channel_id.send_message(&ctx.http, |create_message| {
            create_message.embed(|embed| {
                embed.description(message.to_string(config))
            })
        }).await.map_err(crate::error::Error::Serenity)?;

        if message.is_action() {
            Ok(Some(channel_message))
        } else {
            Ok(None)
        }
    }));

    // Delete the guild's latest action message from before this operation, if this operation
    // sent an action message.
    let old_action_message = guild_model.last_action_message();
    let delete_old_action_message_future = async {
        if maybe_last_action_message_index.is_some() {
            if let Some(old_action_message) = old_action_message {
                old_action_message
                    .channel_id
                    .delete_message(&ctx.http, old_action_message.message_id)
                    .await
                    .map_err(crate::error::Error::Serenity)?;
            }
        }
        Ok(())
    };

    // Execute all the message sending!
    let (_, remaining_messages, _) = futures::try_join!(first_message_future, remaining_messages_future, delete_old_action_message_future)?;

    // Set the guild's last action message to the message we sent, if there was one.
    // If we were expecting an action message but there isn't one collected after sending,
    // the action message was probably sent as the interaction response. This can't be deleted
    // later so we record there being no last action message.
    if maybe_last_action_message_index.is_some() {
        let maybe_sent_message = remaining_messages
            .iter()
            .find_map(|maybe_message| maybe_message.as_ref());
        guild_model.set_last_action_message(maybe_sent_message.map(|sent_message| GuildActionMessage {
            channel_id: sent_message.channel_id,
            message_id: sent_message.id,
        }));
    }

    Ok(())

}
