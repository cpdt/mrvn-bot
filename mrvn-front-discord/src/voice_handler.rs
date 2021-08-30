use serenity::{prelude::*, model::prelude::*};

pub struct VoiceHandler {
    pub client_index: usize,
}

#[serenity::async_trait]
impl EventHandler for VoiceHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        log::info!("Voice client {} is connected as {}", self.client_index, ready.user.name);
    }
}
