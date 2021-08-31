use std::sync::Arc;
use crate::speaker::{Speaker, GuildSpeakerRef, GuildSpeakerHandle};
use serenity::model::prelude::*;
use futures::prelude::*;

pub struct Brain {
    speakers: Vec<Arc<Speaker>>,
}

impl Brain {
    pub fn new() -> Self {
        Brain {
            speakers: Vec::new(),
        }
    }

    pub fn register_speaker(&mut self, speaker: Arc<Speaker>) {
        self.speakers.push(speaker);
    }

    pub fn guild_speakers(&self, guild_id: GuildId) -> BrainSpeakersHandle {
        let guild_speaker_handles: Vec<_> = self.speakers
            .iter()
            .map(|speaker| speaker.get(guild_id))
            .collect();

        BrainSpeakersHandle {
            guild_speaker_handles,
        }
    }
}

pub struct BrainSpeakersHandle {
    guild_speaker_handles: Vec<GuildSpeakerHandle>,
}

impl BrainSpeakersHandle {
    pub async fn lock(&self) -> BrainSpeakersRef<'_> {
        let guild_speaker_refs = future::join_all(self.guild_speaker_handles
            .iter()
            .map(|handle| handle.lock()))
            .await;
        BrainSpeakersRef {
            guild_speaker_refs,
        }
    }
}

pub struct BrainSpeakersRef<'handle> {
    guild_speaker_refs: Vec<GuildSpeakerRef<'handle>>,
}

impl<'handle> BrainSpeakersRef<'handle> {
    pub fn for_channel(&mut self, channel_id: ChannelId) -> Option<&mut GuildSpeakerRef<'handle>> {
        // Look for a speaker already in the channel
        // The weird way of doing this is a workaround for
        // https://users.rust-lang.org/t/solved-borrow-doesnt-drop-returning-this-value-requires-that/24182
        let already_in_channel_index = self.guild_speaker_refs.iter().position(|guild_speaker| guild_speaker.current_channel() == Some(channel_id));
        if let Some(index) = already_in_channel_index {
            return Some(&mut self.guild_speaker_refs[index]);
        }

        // Look for a speaker not in any channel
        let not_in_channel_index = self.guild_speaker_refs.iter().position(|guild_speaker| guild_speaker.current_channel().is_none());
        if let Some(index) = not_in_channel_index {
            return Some(&mut self.guild_speaker_refs[index]);
        }

        // Look for a speaker in a different channel but not active
        let not_active_index = self.guild_speaker_refs.iter().position(|guild_speaker| !guild_speaker.is_active());
        if let Some(index) = not_active_index {
            return Some(&mut self.guild_speaker_refs[index]);
        }

        None
    }
}
