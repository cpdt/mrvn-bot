use crate::{AppModelConfig, GuildModel};
use dashmap::DashMap;
use serenity::model::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AppModel<QueueEntry> {
    config: AppModelConfig,
    guilds: DashMap<GuildId, Arc<Mutex<GuildModel<QueueEntry>>>>,
}

impl<QueueEntry> AppModel<QueueEntry> {
    pub fn new(config: AppModelConfig) -> Self {
        AppModel {
            config,
            guilds: DashMap::new(),
        }
    }

    pub fn get(&self, guild_id: GuildId) -> Arc<Mutex<GuildModel<QueueEntry>>> {
        let handle = self
            .guilds
            .entry(guild_id)
            .or_insert_with(|| Arc::new(Mutex::new(GuildModel::new(self.config))));
        handle.clone()
    }
}
