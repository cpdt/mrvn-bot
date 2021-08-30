use serenity::{prelude::*, model::prelude::*};
use futures::prelude::*;

mod command_handler;
mod commands;
mod config;
mod error;
mod model_delegate;
mod voice_handler;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let mut args = std::env::args();
    let app_name = args.next().unwrap();
    let config_file_path = match args.next() {
        Some(path) => path,
        None => {
            eprintln!("Usage: {} path_to_config.json", app_name);
            std::process::exit(1);
        }
    };

    log::info!("Starting with config from {}", config_file_path);

    let config_file = std::fs::File::open(config_file_path).expect("Unable to open config file");
    let config: config::Config = serde_json::from_reader(config_file).expect("Unable to read config file");

    let mut command_client = Client::builder(&config.command_bot.token)
        .application_id(config.command_bot.application_id)
        .event_handler(command_handler::CommandHandler {
            model: mrvn_model::app_model::AppModel::new(),
        })
        .await
        .expect("Unable to create command client");
    commands::register_commands(
        &command_client.cache_and_http.http,
        config.command_bot.guild_id.map(GuildId)
    ).await.expect("Unable to register commands");
    log::info!("Finished registering application commands");

    log::info!("Starting {} voice clients", config.voice_bots.len());
    let mut voice_clients = future::try_join_all(config
        .voice_bots
        .iter()
        .enumerate()
        .map(|(index, bot_config)| {
            Client::builder(&bot_config.token)
                .application_id(bot_config.application_id)
                .event_handler(voice_handler::VoiceHandler {
                    client_index: index,
                })
        })).await.expect("Unable to create voice client");

    futures::try_join!(
        command_client.start(),
        future::try_join_all(voice_clients.iter_mut().map(|client| client.start())),
    ).expect("Error while running client");
}
