use mrvn_model::AppModelDelegate;
use serenity::{prelude::*, model::prelude::*};

pub struct ModelDelegate {
    guild: Guild,
}

impl ModelDelegate {
    pub async fn new(ctx: &Context, guild_id: GuildId) -> Result<ModelDelegate, crate::error::Error> {
        let guild = ctx.cache.guild(guild_id).await.ok_or(crate::error::Error::UnknownGuild(guild_id))?;
        Ok(ModelDelegate {
            guild,
        })
    }

    pub fn get_user_voice_channel(&self, user_id: UserId) -> Option<ChannelId> {
        self.guild.voice_states.get(&user_id).and_then(|state| state.channel_id)
    }
}

impl AppModelDelegate for ModelDelegate {
    fn is_user_in_voice_channel(&self, user_id: UserId, channel_id: ChannelId) -> bool {
        self.get_user_voice_channel(user_id) == Some(channel_id)
    }
}
