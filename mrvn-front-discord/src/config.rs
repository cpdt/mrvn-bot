use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct CommandBot {
    pub token: String,
    pub application_id: u64,
    pub guild_id: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VoiceBot {
    pub token: String,
    pub application_id: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub command_bot: CommandBot,
    pub voice_bots: Vec<VoiceBot>,
}
