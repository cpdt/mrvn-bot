use serenity::model::id::{ChannelId, MessageId};
use mrvn_back_ytdl::Song;

pub struct QueuedSong {
    pub song: Song,
    pub queue_message_id: Option<(ChannelId, MessageId)>,
}
