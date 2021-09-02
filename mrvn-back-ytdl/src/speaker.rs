use std::sync::Arc;
use serenity::{prelude::*, model::prelude::*};
use serenity::client::ClientBuilder;
use crate::brain::Brain;
use dashmap::DashMap;
use tokio::sync::MutexGuard;
use crate::song::Song;
use std::ops::DerefMut;

pub struct SpeakerKey;

impl TypeMapKey for SpeakerKey {
    type Value = Arc<Speaker>;
}

pub struct Speaker {
    songbird: Arc<songbird::Songbird>,
    guilds: DashMap<GuildId, Arc<Mutex<GuildSpeaker>>>,
}

impl Speaker {
    fn new(songbird: Arc<songbird::Songbird>) -> Self {
        Speaker {
            songbird,
            guilds: DashMap::new(),
        }
    }

    pub fn get(&self, guild_id: GuildId) -> GuildSpeakerHandle {
        let guild_speaker = self.guilds.entry(guild_id)
            .or_insert_with(|| Arc::new(Mutex::new(GuildSpeaker::new())))
            .clone();
        let current_call = self.songbird.get(guild_id);
        GuildSpeakerHandle {
            guild_id,
            songbird: self.songbird.clone(),
            guild_speaker,
            current_call,
        }
    }
}

pub trait SpeakerInit {
    fn register_speaker(self, brain: &mut Brain) -> Self;
}

impl SpeakerInit for ClientBuilder<'_> {
    fn register_speaker(self, brain: &mut Brain) -> Self {
        let songbird = songbird::Songbird::serenity();
        let speaker = Arc::new(Speaker::new(songbird.clone()));
        brain.register_speaker(speaker.clone());

        self
            .voice_manager_arc(songbird)
            .type_map_insert::<SpeakerKey>(speaker)
    }
}

struct GuildSpeaker {
    is_active: bool,
}

impl GuildSpeaker {
    pub fn new() -> Self {
        GuildSpeaker {
            is_active: false,
        }
    }
}

pub struct GuildSpeakerHandle {
    guild_id: GuildId,
    songbird: Arc<songbird::Songbird>,
    guild_speaker: Arc<Mutex<GuildSpeaker>>,
    current_call: Option<Arc<Mutex<songbird::Call>>>,
}

impl GuildSpeakerHandle {
    pub async fn lock(&self) -> GuildSpeakerRef<'_> {
        GuildSpeakerRef {
            guild_id: self.guild_id,
            songbird: &self.songbird,
            guild_speaker_ref: self.guild_speaker.clone(),
            guild_speaker: self.guild_speaker.lock().await,
            current_call: match &self.current_call {
                Some(call_handle) => Some(call_handle.lock().await),
                None => None,
            }
        }
    }
}

pub struct GuildSpeakerRef<'handle> {
    guild_id: GuildId,
    songbird: &'handle songbird::Songbird,
    guild_speaker_ref: Arc<Mutex<GuildSpeaker>>,
    guild_speaker: MutexGuard<'handle, GuildSpeaker>,
    current_call: Option<MutexGuard<'handle, songbird::Call>>,
}

impl<'handle> GuildSpeakerRef<'handle> {
    pub fn current_channel(&self) -> Option<ChannelId> {
        self.current_call
            .as_ref()
            .and_then(|call| call.current_channel().map(|id| ChannelId(id.0)))
    }

    pub fn is_active(&self) -> bool {
        self.guild_speaker.is_active
    }

    pub async fn play<Ended: EndedHandler>(&mut self, channel_id: ChannelId, song: Song, ended_handler: Ended) -> Result<(), crate::error::Error> {
        self.guild_speaker.is_active = true;

        let track_handle = match &mut self.current_call {
            Some(call) if call.current_channel() == Some(channel_id.into()) => {
                play_to_call(call.deref_mut(), song).await
            },
            _ => {
                // Ensure we don't deadlock by having a current_call lock
                self.current_call = None;

                let (call_handle, join_result) = self.songbird.join(self.guild_id, channel_id).await;
                if let Err(why) = join_result {
                    self.guild_speaker.is_active = false;
                    return Err(crate::error::Error::SongbirdJoin(why));
                }

                let mut call = call_handle.lock().await;
                play_to_call(call.deref_mut(), song).await
            }
        };

        track_handle.add_event(songbird::Event::Track(songbird::TrackEvent::End), GuildSpeakerEndedEventHandler {
            ended_handler: Mutex::new(Some(ended_handler)),
            guild_speaker: self.guild_speaker_ref.clone(),
        }).map_err(crate::error::Error::SongbirdTrack)?;

        Ok(())
    }

    pub async fn stop(&mut self) {
        if let Some(call) = &mut self.current_call {
            call.stop();
        }
    }
}

async fn play_to_call(call: &mut songbird::Call, song: Song) -> songbird::tracks::TrackHandle {
   call.play_only_source(song.source())
}

struct GuildSpeakerEndedEventHandler<Ended: EndedHandler> {
    ended_handler: Mutex<Option<Ended>>,
    guild_speaker: Arc<Mutex<GuildSpeaker>>,
}

#[serenity::async_trait]
impl<Ended: EndedHandler> songbird::events::EventHandler for GuildSpeakerEndedEventHandler<Ended> {
    async fn act(&self, _ctx: &songbird::EventContext<'_>) -> Option<songbird::Event> {
        // todo: This opens up a race condition where this speaker can be stolen for another channel
        // before this channel has the chance to start a new song.
        self.guild_speaker.lock().await.is_active = false;

        let mut ended_fn = self.ended_handler.lock().await;
        let old_ended_handler = std::mem::replace(ended_fn.deref_mut(), None);
        if let Some(old_ended_handler) = old_ended_handler {
            old_ended_handler.on_ended();
        }

        None
    }
}

pub trait EndedHandler: Send + 'static {
    fn on_ended(self);
}
