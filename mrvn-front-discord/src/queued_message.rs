use crate::frontend::Frontend;
use crate::message::{Message, ResponseDelegate, ResponseMessage};
use serenity::model::id::{ChannelId, GuildId, MessageId, UserId};
use std::sync::Arc;
use uuid::Uuid;

pub fn build_queued_message(
    frontend: Arc<Frontend>,
    guild_id: GuildId,
    user_id: UserId,
    song_id: Uuid,
    message: ResponseMessage,
) -> Message {
    let delegate = Box::new(QueuedResponseDelegate {
        frontend,
        guild_id,
        user_id,
        song_id,
    });

    Message::Response {
        message,
        delegate: Some(delegate),
    }
}

#[derive(Clone)]
struct QueuedResponseDelegate {
    frontend: Arc<Frontend>,
    guild_id: GuildId,
    user_id: UserId,
    song_id: Uuid,
}

impl ResponseDelegate for QueuedResponseDelegate {
    fn sent(&self, channel_id: ChannelId, message_id: MessageId) {
        let ctx = self.clone();

        tokio::task::spawn(async move {
            let guild_model = ctx.frontend.model.get(ctx.guild_id);
            let mut guild_model_ref = guild_model.lock().await;

            let queued_entry = guild_model_ref.find_user_entry_mut(ctx.user_id, |queued_song| {
                queued_song.song.metadata.id == ctx.song_id
            });
            if let Some(entry) = queued_entry {
                entry.queue_message_id = Some((channel_id, message_id));
            }
        });
    }
}
