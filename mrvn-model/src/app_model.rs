use serenity::model::prelude::*;
use dashmap::DashMap;
use crate::guild_model::GuildModel;
use tokio::sync::Mutex;
use std::ops::DerefMut;
use std::sync::Arc;

pub struct AppModel<QueueEntry> {
    guilds: DashMap<GuildId, Arc<Mutex<GuildModel<QueueEntry>>>>,
}

impl<QueueEntry> AppModel<QueueEntry> {
    pub fn new() -> Self {
        AppModel {
            guilds: DashMap::new(),
        }
    }

    pub fn get(&self, guild_id: GuildId) -> Arc<Mutex<GuildModel<QueueEntry>>> {
        let handle = self.guilds.entry(guild_id)
            .or_insert_with(|| Arc::new(Mutex::new(GuildModel::new())));
        handle.clone()
    }
}
