use crate::config::Config;
use crate::message::ActionMessage;
use serenity::model::id::{ChannelId, MessageId};
use std::sync::Arc;

pub struct ActionUpdater {
    channel_id: ChannelId,
    message_id: MessageId,
    voice_channel: ChannelId,
    is_response: bool,
    config: Arc<Config>,
    http: Arc<serenity::http::Http>,
}

impl ActionUpdater {
    pub fn new(
        channel_id: ChannelId,
        message_id: MessageId,
        voice_channel: ChannelId,
        is_response: bool,
        config: Arc<Config>,
        http: Arc<serenity::http::Http>,
    ) -> Self {
        ActionUpdater {
            channel_id,
            message_id,
            voice_channel,
            is_response,
            config,
            http,
        }
    }

    pub fn is_response(&self) -> bool {
        self.is_response
    }

    pub async fn update(&self, action_message: ActionMessage) {
        let maybe_err = self
            .channel_id
            .edit_message(&self.http, self.message_id, |message| {
                message.embed(|embed| {
                    action_message.create_embed(embed, &self.config, self.voice_channel)
                })
            })
            .await;

        if let Err(why) = maybe_err {
            log::error!("Error while updating action: {}", why);
        }
    }

    pub async fn delete(self) {
        let maybe_err = self
            .channel_id
            .delete_message(&self.http, self.message_id)
            .await;

        if let Err(why) = maybe_err {
            log::error!("Error while deleting action: {}", why);
        };
    }
}
