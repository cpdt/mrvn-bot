use std::sync::Arc;
use serenity::{prelude::*, model::prelude::*};
use serenity::client::ClientBuilder;
use crate::{Brain, Song, SongMetadata, PlayConfig};
use dashmap::DashMap;
use tokio::sync::MutexGuard;
use std::ops::DerefMut;
use std::time::Instant;

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

    pub fn iter(&self) -> impl Iterator<Item=GuildSpeakerHandle> + '_ {
        self.guilds
            .iter()
            .map(move |guild| {
                let guild_id = guild.key().clone();
                let guild_speaker = guild.value().clone();
                let current_call = self.songbird.get(guild_id);
                GuildSpeakerHandle {
                    guild_id,
                    songbird: self.songbird.clone(),
                    guild_speaker,
                    current_call,
                }
            })
    }
}

pub trait SpeakerInit {
    fn register_speaker(self, brain: &mut Brain) -> Self;
}

impl SpeakerInit for ClientBuilder<'_> {
    fn register_speaker(self, brain: &mut Brain) -> Self {
        let songbird = songbird::Songbird::serenity();
        let speaker = Arc::new(Speaker::new(songbird.clone()));
        brain.speakers.push(speaker.clone());

        self
            .voice_manager_arc(songbird)
            .type_map_insert::<SpeakerKey>(speaker)
    }
}

struct GuildPlayingState {
    metadata: SongMetadata,
    track: songbird::tracks::TrackHandle,
    is_paused: bool,
}

struct GuildSpeaker {
    last_ended_time: Option<Instant>,
    playing_state: Option<GuildPlayingState>,
}

impl GuildSpeaker {
    pub fn new() -> Self {
        GuildSpeaker {
            last_ended_time: None,
            playing_state: None,
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
            songbird: self.songbird.clone(),
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
    songbird: Arc<songbird::Songbird>,
    guild_speaker_ref: Arc<Mutex<GuildSpeaker>>,
    guild_speaker: MutexGuard<'handle, GuildSpeaker>,
    current_call: Option<MutexGuard<'handle, songbird::Call>>,
}

impl<'handle> GuildSpeakerRef<'handle> {
    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn last_ended_time(&self) -> Option<Instant> {
        self.guild_speaker.last_ended_time
    }

    pub fn current_channel(&self) -> Option<ChannelId> {
        self.current_call
            .as_ref()
            .and_then(|call| call.current_channel().map(|id| ChannelId(id.0)))
    }

    pub fn is_active(&self) -> bool {
        self.guild_speaker.playing_state.is_some()
    }

    pub fn is_paused(&self) -> bool {
        match &self.guild_speaker.playing_state {
            Some(state) => state.is_paused,
            None => false,
        }
    }

    pub fn active_metadata(&self) -> Option<SongMetadata> {
        self.guild_speaker.playing_state
            .as_ref()
            .map(|state| state.metadata.clone())
    }

    pub async fn play<Ended: EndedHandler>(&mut self, channel_id: ChannelId, song: Song, config: &PlayConfig<'_>, ended_handler: Ended) -> Result<(), crate::error::Error> {
        let input = song.get_input(config).await?;

        let track_handle = match &mut self.current_call {
            Some(call) if call.current_channel() == Some(channel_id.into()) => {
                call.play_only_source(input)
            },
            _ => {
                // Ensure we don't deadlock by having a current_call lock
                self.current_call = None;

                let (call_handle, join_result) = self.songbird.join(self.guild_id, channel_id).await;
                if let Err(why) = join_result {
                    self.guild_speaker.playing_state = None;
                    return Err(crate::error::Error::SongbirdJoin(why));
                }

                let mut call = call_handle.lock().await;
                call.play_only_source(input)
            }
        };

        track_handle.add_event(songbird::Event::Track(songbird::TrackEvent::End), GuildSpeakerEndedEventHandler {
            data: Mutex::new(Some((ended_handler, GuildSpeakerEndedHandle {
                guild_id: self.guild_id,
                songbird: self.songbird.clone(),
                guild_speaker: self.guild_speaker_ref.clone(),
            }))),
        }).map_err(crate::error::Error::SongbirdTrack)?;
        self.guild_speaker.playing_state = Some(GuildPlayingState {
            metadata: song.metadata,
            track: track_handle,
            is_paused: false,
        });

        Ok(())
    }

    pub fn unlock(&mut self) {
        self.guild_speaker.playing_state = None;
        self.guild_speaker.last_ended_time = Some(Instant::now());
    }

    pub fn stop(&mut self) -> Result<(), crate::error::Error> {
        if let Some(playing_state) = &mut self.guild_speaker.playing_state {
            playing_state.track.stop().map_err(crate::error::Error::SongbirdTrack)?;
        }
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), crate::error::Error> {
        if let Some(playing_state) = &mut self.guild_speaker.playing_state {
            playing_state.track.pause().map_err(crate::error::Error::SongbirdTrack)?;
            playing_state.is_paused = true;
        }
        Ok(())
    }

    pub fn unpause(&mut self) -> Result<(), crate::error::Error> {
        if let Some(playing_state) = &mut self.guild_speaker.playing_state {
            playing_state.track.play().map_err(crate::error::Error::SongbirdTrack)?;
            playing_state.is_paused = false;
        }
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<(), crate::error::Error> {
        if let Some(call) = &mut self.current_call {
            call.leave().await.map_err(crate::error::Error::SongbirdJoin)?;
        }
        Ok(())
    }
}

struct GuildSpeakerEndedEventHandler<Ended: EndedHandler> {
    data: Mutex<Option<(Ended, GuildSpeakerEndedHandle)>>,
}

#[serenity::async_trait]
impl<Ended: EndedHandler> songbird::events::EventHandler for GuildSpeakerEndedEventHandler<Ended> {
    async fn act(&self, _ctx: &songbird::EventContext<'_>) -> Option<songbird::Event> {
        let mut data_ref = self.data.lock().await;
        let data = std::mem::replace(data_ref.deref_mut(), None);
        if let Some((ended_handler, handle)) = data {
            ended_handler.on_ended(handle);
        }

        None
    }
}

pub trait EndedHandler: Send + 'static {
    fn on_ended(self, ended_handle: GuildSpeakerEndedHandle);
}

pub struct GuildSpeakerEndedHandle {
    guild_id: GuildId,
    songbird: Arc<songbird::Songbird>,
    guild_speaker: Arc<Mutex<GuildSpeaker>>,
}

impl GuildSpeakerEndedHandle {
    fn get_handle(&self) -> GuildSpeakerHandle {
        GuildSpeakerHandle {
            guild_id: self.guild_id,
            songbird: self.songbird.clone(),
            guild_speaker: self.guild_speaker.clone(),
            current_call: self.songbird.get(self.guild_id),
        }
    }

    pub async fn play<Ended: EndedHandler>(self, channel_id: ChannelId, song: Song, config: &PlayConfig<'_>, ended_handler: Ended) -> Result<(), crate::error::Error> {
        let handle = self.get_handle();
        let mut guild_speaker_ref = handle.lock().await;
        guild_speaker_ref.play(channel_id, song, config, ended_handler).await
    }

    pub async fn stop(self) {
        let handle = self.get_handle();
        let mut guild_speaker_ref = handle.lock().await;
        guild_speaker_ref.guild_speaker.playing_state = None;
        guild_speaker_ref.guild_speaker.last_ended_time = Some(Instant::now());
    }
}
