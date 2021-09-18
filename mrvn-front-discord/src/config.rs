use serde::Deserialize;
use std::collections::HashMap;
use serde::de::Error;

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
    #[serde(deserialize_with = "from_hex")]
    pub embed_color: u32,
    pub skip_votes_required: usize,
    pub stop_votes_required: usize,

    pub disconnect_min_inactive_secs: u64,
    pub disconnect_check_interval_secs: u64,
    pub only_disconnect_when_alone: bool,

    pub command_bot: CommandBot,
    pub voice_bots: Vec<VoiceBot>,
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

fn from_hex<'de, D>(deserializer: D) -> Result<u32, D::Error> where D: serde::Deserializer<'de> {
    let s: String = Deserialize::deserialize(deserializer)?;
    u32::from_str_radix(&s, 16).map_err(D::Error::custom)
}
