use crate::config::Config;
use crate::message::ActionMessage;
use serenity::all::EditMessage;
use serenity::model::id::{ChannelId, MessageId};
use serenity::prelude::Context;
use std::sync::Arc;

pub struct ActionUpdater {
    channel_id: ChannelId,
    message_id: MessageId,
    voice_channel: ChannelId,
    is_response: bool,
    config: Arc<Config>,
    ctx: Context,
}

impl ActionUpdater {
    pub fn new(
        channel_id: ChannelId,
        message_id: MessageId,
        voice_channel: ChannelId,
        is_response: bool,
        config: Arc<Config>,
        ctx: Context,
    ) -> Self {
        ActionUpdater {
            channel_id,
            message_id,
            voice_channel,
            is_response,
            config,
            ctx,
        }
    }

    pub fn is_response(&self) -> bool {
        self.is_response
    }

    pub async fn update(&self, action_message: ActionMessage) {
        let maybe_err = self
            .channel_id
            .edit_message(
                &self.ctx,
                self.message_id,
                EditMessage::new()
                    .embed(action_message.create_embed(&self.config, self.voice_channel)),
            )
            .await;

        if let Err(why) = maybe_err {
            log::error!("Error while updating action: {}", why);
        }
    }

    pub async fn delete(self) {
        let maybe_err = self
            .channel_id
            .delete_message(&self.ctx.http, self.message_id)
            .await;

        if let Err(why) = maybe_err {
            log::error!("Error while deleting action: {}", why);
        };
    }
}
