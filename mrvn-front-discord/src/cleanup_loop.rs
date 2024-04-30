use crate::config::Config;
use crate::frontend::Frontend;
use futures::future;
use mrvn_back_ytdl::GuildSpeakerHandle;
use std::sync::Arc;
use std::time::Duration;

async fn check_cleanup_for_speaker(
    guild_speaker_handle: GuildSpeakerHandle,
    cache: Arc<serenity::cache::Cache>,
    config: Arc<Config>,
) {
    let mut guild_speaker = guild_speaker_handle.lock().await;

    // Ignore the speaker if it's currently active or not even connected
    if guild_speaker.is_active() {
        return;
    }

    // Ignore the speaker if it's not connected
    let channel_id = match guild_speaker.current_channel() {
        Some(channel) => channel,
        None => return,
    };

    // Ignore the channel if it hasn't played yet
    let last_ended_time = match guild_speaker.last_ended_time() {
        Some(time) => time,
        None => return,
    };

    // Ignore the speaker if not enough time has passed since last playback
    if last_ended_time.elapsed().as_secs() < config.disconnect_min_inactive_secs {
        return;
    }

    if config.only_disconnect_when_alone {
        let maybe_guild = cache.guild(guild_speaker.guild_id());
        let maybe_member_count = maybe_guild.map(|guild| guild.voice_states.values().filter(|voice_state| voice_state.channel_id == Some(channel_id)).count());

        if let Some(member_count) = maybe_member_count {
            // Our bot counts as a member, so don't disconnect if there's more than just it.
            if member_count > 1 {
                return;
            }
        }
    }

    // We've passed the conditions, disconnect
    match guild_speaker.disconnect().await {
        Ok(_) => log::debug!("Disconnected speaker due to inactivity"),
        Err(why) => log::error!("Error when disconnecting speaker: {}", why),
    }
}

async fn check_cleanup(frontend: Arc<Frontend>, cache: Arc<serenity::cache::Cache>) {
    let futures = frontend
        .backend_brain
        .speakers
        .iter()
        .flat_map(|speaker| speaker.iter())
        .map(|guild_speaker_handle| {
            check_cleanup_for_speaker(guild_speaker_handle, cache.clone(), frontend.config.clone())
        });

    future::join_all(futures).await;
}

pub async fn cleanup_loop(frontend: Arc<Frontend>, cache: Arc<serenity::cache::Cache>) -> ! {
    let mut interval = tokio::time::interval(Duration::from_secs(
        frontend.config.disconnect_check_interval_secs,
    ));
    loop {
        interval.tick().await;
        tokio::task::spawn(check_cleanup(frontend.clone(), cache.clone()));
    }
}
