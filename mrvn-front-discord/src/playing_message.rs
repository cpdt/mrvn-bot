use crate::frontend::Frontend;
use crate::message::time_bar::{format_time, AFTER_PROGRESS_BAR, BEFORE_PROGRESS_BAR, MAX_COLUMNS};
use crate::message::{ActionDelegate, ActionMessage, ActionUpdater, Message};
use futures::future::{AbortHandle, Abortable};
use mrvn_back_ytdl::{GuildSpeakerRef, SongMetadata};
use serenity::model::id::{ChannelId, GuildId};
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, MissedTickBehavior};

fn get_playing_action_message_at_time(
    is_response: bool,
    channel_id: ChannelId,
    current_metadata: &SongMetadata,
    play_time: Option<Duration>,
) -> ActionMessage {
    let time_seconds = play_time.map(|time| time.as_secs_f64()).unwrap_or(0.);

    if is_response {
        ActionMessage::PlayingResponse {
            song_title: current_metadata.title.clone(),
            song_url: current_metadata.url.clone(),
            voice_channel_id: channel_id,
            thumbnail_url: current_metadata.thumbnail_url.clone(),
            time_seconds,
            duration_seconds: current_metadata.duration_seconds,
        }
    } else {
        ActionMessage::Playing {
            song_title: current_metadata.title.clone(),
            song_url: current_metadata.url.clone(),
            voice_channel_id: channel_id,
            user_id: current_metadata.user_id,
            thumbnail_url: current_metadata.thumbnail_url.clone(),
            time_seconds,
            duration_seconds: current_metadata.duration_seconds,
        }
    }
}

fn get_played_action_message(current_metadata: &SongMetadata) -> ActionMessage {
    ActionMessage::Played {
        song_title: current_metadata.title.clone(),
        song_url: current_metadata.url.clone(),
    }
}

async fn get_action_message(
    is_response: bool,
    channel_id: ChannelId,
    current_metadata: &SongMetadata,
    speaker_ref: &GuildSpeakerRef<'_>,
) -> ActionMessage {
    let play_time = speaker_ref.active_play_time().await;
    get_playing_action_message_at_time(is_response, channel_id, current_metadata, play_time)
}

pub async fn build_playing_message(
    frontend: Arc<Frontend>,
    speaker_ref: &GuildSpeakerRef<'_>,
    is_response: bool,
    channel_id: ChannelId,
    current_metadata: SongMetadata,
) -> Message {
    let initial_action_message =
        get_action_message(is_response, channel_id, &current_metadata, speaker_ref).await;
    let delegate = Box::new(PlayingActionDelegate {
        frontend,

        is_response,
        guild_id: speaker_ref.guild_id(),
        initial_channel_id: channel_id,
        song_metadata: current_metadata,
    });

    Message::Action {
        message: initial_action_message,
        voice_channel: channel_id,
        delegate: Some(delegate),
    }
}

struct PlayingActionDelegate {
    frontend: Arc<Frontend>,

    is_response: bool,
    guild_id: GuildId,
    initial_channel_id: ChannelId,
    song_metadata: SongMetadata,
}

impl ActionDelegate for PlayingActionDelegate {
    fn start(&self, updater: ActionUpdater) -> Box<dyn Any + Send + Sync> {
        let metadata = ActivePlayingActionMetadata {
            updater: Some(updater),
            frontend: self.frontend.clone(),

            is_response: self.is_response,
            guild_id: self.guild_id,
            song_metadata: self.song_metadata.clone(),

            current_channel_id: self.initial_channel_id,
        };

        let (abort, abort_registration) = AbortHandle::new_pair();
        tokio::spawn(Abortable::new(
            update_playing_message_loop(metadata),
            abort_registration,
        ));

        Box::new(ActivePlayingActionDelegate { abort })
    }
}

struct ActivePlayingActionMetadata {
    updater: Option<ActionUpdater>,
    frontend: Arc<Frontend>,

    is_response: bool,
    guild_id: GuildId,
    song_metadata: SongMetadata,

    current_channel_id: ChannelId,
}

struct ActivePlayingActionDelegate {
    abort: AbortHandle,
}

impl Drop for ActivePlayingActionDelegate {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

impl Drop for ActivePlayingActionMetadata {
    fn drop(&mut self) {
        if let Some(updater) = std::mem::take(&mut self.updater) {
            if self.is_response {
                let final_message = get_played_action_message(&self.song_metadata);
                tokio::task::spawn(async move {
                    updater.update(final_message).await;
                });
            } else {
                tokio::task::spawn(updater.delete());
            }
        }
    }
}

async fn update_playing_message_loop(mut metadata: ActivePlayingActionMetadata) {
    let min_update_secs = metadata.frontend.config.progress_min_update_secs;
    let max_update_secs = metadata.frontend.config.progress_max_update_secs;

    // Guess how often we'd need to tick to update one piece of the progress bar each time
    let update_period_secs = match metadata.song_metadata.duration_seconds {
        Some(duration) => {
            let time_width = format_time(&metadata.frontend.config, 0., Some(duration)).len();
            let progress_width =
                (MAX_COLUMNS - time_width - BEFORE_PROGRESS_BAR.len() - AFTER_PROGRESS_BAR.len())
                    .max(1);
            (duration / progress_width as f64).clamp(min_update_secs, max_update_secs)
        }
        None => max_update_secs,
    };
    let period_duration = Duration::from_secs_f64(update_period_secs);

    let mut interval = tokio::time::interval_at(Instant::now() + period_duration, period_duration);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        let updater = match &metadata.updater {
            Some(updater) => updater,
            None => return,
        };

        let action_message = {
            let guild_speakers = metadata
                .frontend
                .backend_brain
                .guild_speakers(metadata.guild_id);
            let mut guild_speakers_ref = guild_speakers.lock().await;

            let (active_speaker, active_metadata) =
                match guild_speakers_ref.find_active_song(metadata.song_metadata.id) {
                    Some(val) => val,
                    None => {
                        // The song has ended, returning will drop the metadata and clear the message.
                        return;
                    }
                };

            if let Some(channel) = active_speaker.current_channel() {
                metadata.current_channel_id = channel;
            }

            get_action_message(
                metadata.is_response,
                metadata.current_channel_id,
                &active_metadata,
                active_speaker,
            )
            .await
        };
        updater.update(action_message).await;
    }
}
