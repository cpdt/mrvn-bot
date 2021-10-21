use crate::frontend::Frontend;
use serenity::{model::prelude::*, prelude::*};
use std::sync::Arc;

pub struct CommandHandler {
    frontend: Arc<Frontend>,
}

impl CommandHandler {
    pub fn new(frontend: Arc<Frontend>) -> Self {
        CommandHandler { frontend }
    }
}

#[serenity::async_trait]
impl EventHandler for CommandHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        log::info!("Command client is connected as {}", ready.user.name);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            self.frontend.handle_command(&ctx, &command).await;
        }
    }
}
