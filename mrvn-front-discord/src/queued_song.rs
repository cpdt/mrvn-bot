use mrvn_back_ytdl::Song;
use serenity::model::id::{ChannelId, MessageId};

pub struct QueuedSong {
    pub song: Song,
    pub queue_message_id: Option<(ChannelId, MessageId)>,
}
