use mrvn_back_ytdl::PlayConfig;
use serde::de::Error;
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
pub struct YtdlConfig {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FfmpegConfig {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecretHighfive {
    pub image_url: String,
    pub timezone: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(deserialize_with = "from_hex")]
    pub action_embed_color: u32,
    #[serde(deserialize_with = "from_hex")]
    pub response_embed_color: u32,
    #[serde(deserialize_with = "from_hex")]
    pub error_embed_color: u32,

    pub skip_votes_required: usize,
    pub stop_votes_required: usize,

    pub disconnect_min_inactive_secs: u64,
    pub disconnect_check_interval_secs: u64,
    pub only_disconnect_when_alone: bool,

    pub search_prefix: String,
    pub host_blocklist: Vec<String>,
    pub ytdl: YtdlConfig,
    pub ffmpeg: FfmpegConfig,

    pub command_bot: CommandBot,
    pub voice_bots: Vec<VoiceBot>,
    pub messages: HashMap<String, String>,

    pub secret_highfive: Option<SecretHighfive>,
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

        SUBSTITUTE_REGEX
            .replace_all(message_template, |caps: &regex::Captures| {
                let substitute_name = &caps[1];
                substitutions
                    .iter()
                    .find(|(key, _)| *key == substitute_name)
                    .map(|(_, value)| *value)
                    .unwrap_or("")
            })
            .into_owned()
    }

    pub fn get_play_config(&self) -> PlayConfig {
        PlayConfig {
            search_prefix: &self.search_prefix,
            host_blocklist: &self.host_blocklist,
            ytdl_name: &self.ytdl.name,
            ytdl_args: &self.ytdl.args,
            ffmpeg_name: &self.ffmpeg.name,
            ffmpeg_args: &self.ffmpeg.args,
        }
    }
}

fn from_hex<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    u32::from_str_radix(&s, 16).map_err(D::Error::custom)
}
