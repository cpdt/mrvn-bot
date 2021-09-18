use serenity::model::prelude::*;
use futures::prelude::*;

async fn delete_all_global_application_commands(http: impl AsRef<serenity::http::Http>) -> serenity::Result<()> {
    let http_ref = http.as_ref();
    let commands = http_ref.get_global_application_commands().await?;
    log::trace!("Deleting {} global application commands", commands.len());
    future::try_join_all(commands.iter().map(|command| http_ref.delete_global_application_command(command.id.0))).await?;
    Ok(())
}

fn play_command(command: &mut serenity::builder::CreateApplicationCommand) -> &mut serenity::builder::CreateApplicationCommand {
    command
        .name("play")
        .description("Add a song to your queue.")
        .create_option(|option| {
            option
                .name("term")
                .description("A search term or song link.")
                .kind(application_command::ApplicationCommandOptionType::String)
                .required(false)
        })
}

//fn queue_command(command: &mut serenity::builder::CreateApplicationCommand) -> &mut serenity::builder::CreateApplicationCommand {
//    command
//        .name("queue")
//        .description("View and manage your queue.")
//}

fn replace_command(command: &mut serenity::builder::CreateApplicationCommand) -> &mut serenity::builder::CreateApplicationCommand {
    command
        .name("replace")
        .description("Replace your most recent song with a different one.")
        .create_option(|option| {
            option
                .name("term")
                .description("A search term or song link.")
                .kind(application_command::ApplicationCommandOptionType::String)
                .required(true)
        })
}

fn pause_command(command: &mut serenity::builder::CreateApplicationCommand) -> &mut serenity::builder::CreateApplicationCommand {
    command
        .name("pause")
        .description("Pause the current song.")
}

fn skip_command(command: &mut serenity::builder::CreateApplicationCommand) -> &mut serenity::builder::CreateApplicationCommand {
    command
        .name("skip")
        .description("Vote to skip the current song.")
}

fn stop_command(command: &mut serenity::builder::CreateApplicationCommand) -> &mut serenity::builder::CreateApplicationCommand {
    command
        .name("stop")
        .description("Vote to skip the current song and stop playback.")
}

pub async fn register_commands(http: impl AsRef<serenity::http::Http>, guild_id: Option<GuildId>) -> serenity::Result<()> {
    let http_ref = http.as_ref();
    match guild_id {
        Some(guild_id) => {
            delete_all_global_application_commands(http_ref).await?;
            log::trace!("Registering guild application commands");
            futures::try_join!(
                guild_id.create_application_command(http_ref, play_command),
                // guild_id.create_application_command(http_ref, queue_command),
                guild_id.create_application_command(http_ref, replace_command),
                guild_id.create_application_command(http_ref, pause_command),
                guild_id.create_application_command(http_ref, skip_command),
                guild_id.create_application_command(http_ref, stop_command),
            )?;
        },
        None => {
            log::trace!("Registering global application commands");
            application_command::ApplicationCommand::set_global_application_commands(http_ref, |commands| {
                commands
                    .create_application_command(play_command)
                    .create_application_command(replace_command)
                    .create_application_command(pause_command)
                    .create_application_command(skip_command)
                    .create_application_command(stop_command)
            }).await?;
        }
    };

    Ok(())
}
