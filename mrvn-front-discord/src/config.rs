use serde::Deserialize;
use std::collections::HashMap;

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
    pub skip_votes_required: usize,
    pub messages: HashMap<String, String>,
}

impl Config {
    pub fn get_raw_message<'s>(&'s self, message_key: &'s str) -> &'s str {
        match self.messages.get(message_key) {
            Some(template) => template,
            None => {
                log::warn!("Message string {} was not included in config", message_key);
                message_key
            }
        }
    }

    pub fn get_message(&self, message_key: &str, substitutions: &[(&str, &str)]) -> String {
        let message_template = self.get_raw_message(message_key);

        lazy_static::lazy_static! {
            static ref SUBSTITUTE_REGEX: regex::Regex = regex::Regex::new(r"\{(\w+)\}").unwrap();
        }

        SUBSTITUTE_REGEX.replace_all(message_template, |caps: &regex::Captures| {
            let substitute_name = &caps[1];
            substitutions
                .iter()
                .find(|(key, _)| *key == substitute_name)
                .map(|(_, value)| *value)
                .unwrap_or("")
        }).into_owned()
    }
}
