use serenity::model::prelude::*;
use dashmap::DashMap;
use crate::guild_model::GuildModel;
use tokio::sync::Mutex;
use std::ops::DerefMut;

pub struct AppModel<QueueEntry> {
    guilds: DashMap<GuildId, Mutex<GuildModel<QueueEntry>>>,
}

impl<QueueEntry> AppModel<QueueEntry> {
    pub fn new() -> Self {
        AppModel {
            guilds: DashMap::new(),
        }
    }

    pub async fn get<Ret, Fn: FnOnce(&mut GuildModel<QueueEntry>) -> Ret>(&self, guild_id: GuildId, f: Fn) -> Ret {
        let mut handle = self.guilds.entry(guild_id)
            .or_insert_with(|| Mutex::new(GuildModel::new()));
        let mut guild = handle.value_mut().lock().await;
        f(guild.deref_mut())
    }
}
