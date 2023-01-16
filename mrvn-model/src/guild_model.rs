use crate::AppModelConfig;
use serenity::model::prelude::*;
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};

fn find_first_user_in_channel<'a, Entry: 'a>(
    cache: &serenity::cache::Cache,
    mut queues: impl Iterator<Item = &'a Queue<Entry>>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Option<UserId> {
    cache
        .guild_field(guild_id, |guild| {
            queues
                .find(|queue| {
                    let current_channel = guild
                        .voice_states
                        .get(&queue.user_id)
                        .and_then(|voice_state| voice_state.channel_id);
                    current_channel == Some(channel_id)
                })
                .map(|queue| queue.user_id)
        })
        .flatten()
}

fn is_user_in_voice_channel(
    cache: &serenity::cache::Cache,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
) -> bool {
    cache
        .guild_field(guild_id, |guild| {
            let current_channel = guild
                .voice_states
                .get(&user_id)
                .and_then(|voice_state| voice_state.channel_id);
            current_channel == Some(channel_id)
        })
        .unwrap_or(false)
}

pub enum VoteType {
    Skip,
    Stop,
}

pub enum VoteStatus {
    Success,
    AlreadyVoted,
    NeedsMoreVotes(usize),
    NothingPlaying,
}

pub enum ReplaceStatus<QueueEntry> {
    Queued,
    ReplacedInQueue(QueueEntry),
    ReplacedCurrent(ChannelId),
}

pub enum NextEntry<QueueEntry> {
    NoneAvailable,
    AlreadyPlaying,
    Entry(QueueEntry),
}

struct Queue<Entry> {
    user_id: UserId,
    entries: VecDeque<Entry>,
}

enum ChannelPlayingState {
    NotPlaying,
    Stopped,
    Playing {
        playing_user_id: UserId,
        skip_votes: HashSet<UserId>,
        stop_votes: HashSet<UserId>,
    },
}

impl ChannelPlayingState {
    fn is_playing(&self) -> bool {
        matches!(self, ChannelPlayingState::Playing { .. })
    }
}

pub struct ChannelActionMessage {
    pub frontend_handle: Box<dyn Any + Send + Sync>,
}

struct ChannelModel {
    playing: ChannelPlayingState,
    last_action_message: Option<ChannelActionMessage>,
}

pub struct GuildModel<QueueEntry> {
    guild_id: GuildId,
    config: AppModelConfig,
    message_channel: Option<ChannelId>,
    queues: Vec<Queue<QueueEntry>>,
    channels: HashMap<ChannelId, ChannelModel>,
}

impl<QueueEntry> GuildModel<QueueEntry> {
    pub fn new(guild_id: GuildId, config: AppModelConfig) -> Self {
        GuildModel {
            guild_id,
            config,
            message_channel: None,
            queues: Vec::new(),
            channels: HashMap::new(),
        }
    }

    pub fn message_channel(&self) -> Option<ChannelId> {
        self.message_channel
    }

    pub fn set_message_channel(&mut self, message_channel: Option<ChannelId>) {
        self.message_channel = message_channel;
    }

    pub fn clear_last_action_message(
        &mut self,
        channel_id: ChannelId,
    ) -> Option<ChannelActionMessage> {
        self.channels
            .get_mut(&channel_id)
            .and_then(|channel| std::mem::take(&mut channel.last_action_message))
    }

    pub fn set_last_action_message(
        &mut self,
        channel_id: ChannelId,
        status_message: Option<ChannelActionMessage>,
    ) {
        self.create_channel(channel_id).last_action_message = status_message;
    }

    pub fn is_channel_stopped(&self, channel_id: ChannelId) -> bool {
        matches!(
            self.get_channel_playing_state(channel_id),
            Some(ChannelPlayingState::Stopped)
        )
    }

    pub fn set_channel_stopped(&mut self, channel_id: ChannelId) {
        self.create_channel(channel_id).playing = ChannelPlayingState::Stopped;
    }

    pub fn find_user_entry_mut(
        &mut self,
        user_id: UserId,
        mut f: impl FnMut(&QueueEntry) -> bool,
    ) -> Option<&mut QueueEntry> {
        if let Some(queue) = self.get_user_queue_mut(user_id) {
            queue.entries.iter_mut().find(|entry| f(*entry))
        } else {
            None
        }
    }

    // User commands:
    pub fn push_entries(&mut self, user_id: UserId, entries: impl IntoIterator<Item = QueueEntry>) {
        let queue = self.create_user_queue(user_id);
        queue.entries.extend(entries);
    }

    pub fn replace_entry(
        &mut self,
        user_id: UserId,
        maybe_channel_id: Option<ChannelId>,
        entry: QueueEntry,
    ) -> ReplaceStatus<QueueEntry> {
        let queue = self.create_user_queue(user_id);
        let removed_entry = queue.entries.pop_back();
        queue.entries.push_back(entry);

        match removed_entry {
            Some(entry) => ReplaceStatus::ReplacedInQueue(entry),
            None => {
                // If the current channel is playing this user, the current song should be skipped.
                if let Some(channel_id) = maybe_channel_id {
                    let maybe_playing_user = self.get_channel_playing_user(channel_id);
                    if maybe_playing_user == Some(user_id) {
                        return ReplaceStatus::ReplacedCurrent(channel_id);
                    }
                }

                ReplaceStatus::Queued
            }
        }
    }

    // Events:
    pub fn next_channel_entry_finished(
        &mut self,
        cache: &serenity::cache::Cache,
        channel_id: ChannelId,
    ) -> Option<QueueEntry> {
        let old_playing_state = std::mem::replace(
            &mut self.create_channel(channel_id).playing,
            ChannelPlayingState::NotPlaying,
        );

        // Round-robin to the next user
        let next_user_id = match old_playing_state {
            ChannelPlayingState::Playing {
                playing_user_id: user_id,
                ..
            } => {
                let last_playing_queue_index = self
                    .queues
                    .iter_mut()
                    .position(|queue| queue.user_id == user_id);
                match last_playing_queue_index {
                    Some(last_playing_index) => {
                        // Search queues from after the last active one, back around to it again
                        let queues_iter = self
                            .queues
                            .iter()
                            .skip(last_playing_index + 1)
                            .chain(self.queues.iter().take(last_playing_index + 1));
                        find_first_user_in_channel(cache, queues_iter, self.guild_id, channel_id)
                    }
                    None => find_first_user_in_channel(
                        cache,
                        self.queues.iter(),
                        self.guild_id,
                        channel_id,
                    ),
                }
            }
            _ => find_first_user_in_channel(cache, self.queues.iter(), self.guild_id, channel_id),
        }?;

        let next_queue = self.get_user_queue_mut(next_user_id)?;
        let next_entry = next_queue.entries.pop_front()?;

        // Update channel state to indicate it's playing
        self.create_channel(channel_id).playing = ChannelPlayingState::Playing {
            playing_user_id: next_queue.user_id,
            skip_votes: HashSet::new(),
            stop_votes: HashSet::new(),
        };

        // Remove any empty queues and channels
        self.queues.retain(|queue| !queue.entries.is_empty());
        self.channels
            .retain(|_, channel| channel.playing.is_playing());

        Some(next_entry)
    }

    pub fn next_channel_entry(
        &mut self,
        cache: &serenity::cache::Cache,
        channel_id: ChannelId,
    ) -> NextEntry<QueueEntry> {
        match self.get_channel_playing_state(channel_id) {
            Some(ChannelPlayingState::Playing { .. }) => NextEntry::AlreadyPlaying,
            _ => match self.next_channel_entry_finished(cache, channel_id) {
                Some(entry) => NextEntry::Entry(entry),
                None => NextEntry::NoneAvailable,
            },
        }
    }

    pub fn vote_for_skip(
        &mut self,
        cache: &serenity::cache::Cache,
        vote_type: VoteType,
        channel_id: ChannelId,
        user_id: UserId,
    ) -> VoteStatus {
        let votes_required = match vote_type {
            VoteType::Skip => self.config.skip_votes_required,
            VoteType::Stop => self.config.stop_votes_required,
        };
        let guild_id = self.guild_id;
        match self.get_channel_playing_state_mut(channel_id) {
            Some(ChannelPlayingState::Playing {
                playing_user_id,
                skip_votes,
                stop_votes,
                ..
            }) => {
                let votes = match vote_type {
                    VoteType::Skip => skip_votes,
                    VoteType::Stop => stop_votes,
                };

                // We can skip immediately if this was the user who's currently playing
                if user_id == *playing_user_id {
                    return VoteStatus::Success;
                }

                // We can skip immediately if the user who played this entry is not in the channel
                // anymore.
                if !is_user_in_voice_channel(cache, guild_id, channel_id, user_id) {
                    return VoteStatus::Success;
                }

                // Prevent voting if this user has already voted
                if votes.contains(&user_id) {
                    return VoteStatus::AlreadyVoted;
                }

                // We can succeed immediately if we will have the required number of votes
                if votes.len() + 1 >= votes_required {
                    return VoteStatus::Success;
                }

                // Add the vote and indicate more votes are needed
                votes.insert(user_id);
                VoteStatus::NeedsMoreVotes(votes_required - votes.len())
            }
            _ => VoteStatus::NothingPlaying,
        }
    }

    fn get_user_queue_mut(&mut self, user_id: UserId) -> Option<&mut Queue<QueueEntry>> {
        self.queues
            .iter_mut()
            .find(|queue| queue.user_id == user_id)
    }

    fn create_user_queue(&mut self, user_id: UserId) -> &mut Queue<QueueEntry> {
        // For some reason we need to get the index then lookup instead of using .find() to work
        // around the borrow checker.
        if let Some(existing_queue_index) = self
            .queues
            .iter()
            .position(|queue| queue.user_id == user_id)
        {
            return &mut self.queues[existing_queue_index];
        }

        self.queues.push(Queue {
            user_id,
            entries: VecDeque::new(),
        });
        self.queues.last_mut().unwrap()
    }

    fn create_channel(&mut self, channel_id: ChannelId) -> &mut ChannelModel {
        self.channels.entry(channel_id).or_insert(ChannelModel {
            playing: ChannelPlayingState::NotPlaying,
            last_action_message: None,
        })
    }

    fn get_channel_playing_state(&self, channel_id: ChannelId) -> Option<&ChannelPlayingState> {
        self.channels
            .get(&channel_id)
            .map(|channel| &channel.playing)
    }

    fn get_channel_playing_state_mut(
        &mut self,
        channel_id: ChannelId,
    ) -> Option<&mut ChannelPlayingState> {
        self.channels
            .get_mut(&channel_id)
            .map(|channel| &mut channel.playing)
    }

    fn get_channel_playing_user(&self, channel_id: ChannelId) -> Option<UserId> {
        match self.get_channel_playing_state(channel_id) {
            Some(ChannelPlayingState::Playing {
                playing_user_id: user_id,
                ..
            }) => Some(*user_id),
            _ => None,
        }
    }
}
