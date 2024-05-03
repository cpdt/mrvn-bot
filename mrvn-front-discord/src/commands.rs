use serenity::all::{CreateCommand, CreateCommandOption};
use serenity::model::prelude::*;

pub async fn register_commands(
    http: impl AsRef<serenity::http::Http>,
    guild_id: Option<GuildId>,
) -> serenity::Result<()> {
    let http_ref = http.as_ref();

    let commands = vec![
        CreateCommand::new("play")
            .description("Add a song to your queue.")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "term",
                    "A search term or song link.",
                )
                .required(true),
            ),
        CreateCommand::new("resume").description("Resume a paused song."),
        CreateCommand::new("replace")
            .description("Replace your most recent song with a different one.")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "term",
                    "A search term or song link.",
                )
                .required(true),
            ),
        CreateCommand::new("pause").description("Pause the current song."),
        CreateCommand::new("skip").description("Vote to skip the current song."),
        CreateCommand::new("stop").description("Vote to skip the current song and stop playback."),
        CreateCommand::new("nowplaying")
            .description("View the current playing song and its progress."),
    ];

    match guild_id {
        Some(guild_id) => {
            Command::set_global_commands(http_ref, Vec::new()).await?;
            guild_id.set_commands(http_ref, commands).await?;
        }
        None => {
            log::trace!("Registering global application commands");
            Command::set_global_commands(http_ref, commands).await?;
        }
    }

    Ok(())
}
