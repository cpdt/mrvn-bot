use serenity::model::prelude::*;

pub trait AppModelDelegate {
    fn is_user_in_voice_channel(&self, user_id: UserId, channel_id: ChannelId) -> bool;
}
