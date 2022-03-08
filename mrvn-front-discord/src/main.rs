use futures::prelude::*;
use mrvn_back_ytdl::SpeakerInit;
use serenity::{model::prelude::*, prelude::*};
use std::sync::Arc;

mod cleanup_loop;
mod command_handler;
mod commands;
mod config;
mod error;
mod frontend;
mod message;
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
    let config: Arc<config::Config> =
        Arc::new(serde_json::from_reader(config_file).expect("Unable to read config file"));

    let mut backend_brain = mrvn_back_ytdl::Brain::new();
    let model = mrvn_model::AppModel::new(mrvn_model::AppModelConfig {
        skip_votes_required: config.skip_votes_required,
        stop_votes_required: config.stop_votes_required,

        secret_highfive_timezone: match &config.secret_highfive {
            Some(secret) => secret.timezone.parse().expect("Unable to parse timezone"),
            None => chrono_tz::Etc::UTC,
        },
    });

    log::info!("Starting {} voice clients", config.voice_bots.len());
    let mut voice_clients = future::try_join_all(config.voice_bots.iter().enumerate().map(
        |(index, bot_config)| {
            Client::builder(&bot_config.token)
                .application_id(bot_config.application_id)
                .event_handler(voice_handler::VoiceHandler {
                    client_index: index,
                })
                .register_speaker(&mut backend_brain)
        },
    ))
    .await
    .expect("Unable to create voice client");

    let frontend = Arc::new(crate::frontend::Frontend::new(
        config.clone(),
        backend_brain,
        model,
    ));
    let mut command_client = Client::builder(&config.command_bot.token)
        .application_id(config.command_bot.application_id)
        .event_handler(command_handler::CommandHandler::new(frontend.clone()))
        .await
        .expect("Unable to create command client");
    commands::register_commands(
        &command_client.cache_and_http.http,
        config.command_bot.guild_id.map(GuildId),
        &config,
    )
    .await
    .expect("Unable to register commands");
    log::info!("Finished registering application commands");

    let cleanup_loop_future =
        cleanup_loop::cleanup_loop(frontend, command_client.cache_and_http.cache.clone())
            .map(|_| Ok(()));

    futures::try_join!(
        command_client.start(),
        future::try_join_all(voice_clients.iter_mut().map(|client| client.start())),
        cleanup_loop_future,
    )
    .expect("Error while running client");
}
