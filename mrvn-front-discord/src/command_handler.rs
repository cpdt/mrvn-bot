use serenity::{prelude::*, model::prelude::*};
use std::sync::Arc;
use crate::frontend::Frontend;

fn unknown_error_message() -> &'static str {
    ":robot: :weary: An error occurred."
}

pub struct CommandHandler {
    frontend: Arc<Frontend>,
}

impl CommandHandler {
    pub fn new(frontend: Frontend) -> Self {
        CommandHandler {
            frontend: Arc::new(frontend),
        }
    }
}

#[serenity::async_trait]
impl EventHandler for CommandHandler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        log::info!("Command client is connected as {}", ready.user.name);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            if let Err(why) = command.create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
            }).await {
                log::error!("Error while sending deferred message: {}", why);
            }

            if let Err(why) = self.frontend.clone().handle_command(ctx.clone(), &command).await {
                log::error!("Error while handling command: {}", why);
                let edit_res = command.edit_original_interaction_response(&ctx.http, |response| {
                    response.create_embed(|embed| {
                        embed.description(unknown_error_message())
                    })
                }).await;
                if let Err(why) = edit_res {
                    log::error!("Error while sending error response: {}", why);
                }
            }
        }
    }
}
