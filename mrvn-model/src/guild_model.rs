use serenity::model::prelude::*;
use std::collections::{VecDeque, HashMap, HashSet};
use crate::app_model_delegate::AppModelDelegate;
use crate::config::AppModelConfig;

fn find_first_user_in_channel<'a, Entry: 'a, Delegate: AppModelDelegate>(mut queues: impl Iterator<Item=&'a Queue<Entry>>, delegate: &Delegate, channel_id: ChannelId) -> Option<UserId> {
    queues
        .find(|queue| delegate.is_user_in_voice_channel(queue.user_id, channel_id))
        .map(|queue| queue.user_id)
}

pub enum SkipStatus {
    OkToSkip,
    AlreadyVoted,
    NeedsMoreVotes(usize),
    NothingPlaying,
}

struct Queue<Entry> {
    user_id: UserId,
    entries: VecDeque<Entry>,
}

struct ChannelPlayingState {
    user_id: UserId,
    skip_votes: HashSet<UserId>,
}

struct ChannelModel {
    playing: Option<ChannelPlayingState>,
}

pub struct StatusMessage {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
}

pub struct GuildModel<QueueEntry> {
    config: AppModelConfig,
    message_channel: Option<ChannelId>,
    last_status_message: Option<StatusMessage>,
    queues: Vec<Queue<QueueEntry>>,
    channels: HashMap<ChannelId, ChannelModel>,
}

impl<QueueEntry> GuildModel<QueueEntry> {
    pub fn new(config: AppModelConfig) -> Self {
        GuildModel {
            config,
            message_channel: None,
            last_status_message: None,
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

    pub fn swap_last_status_message(&mut self, last_status_message: Option<StatusMessage>) -> Option<StatusMessage> {
        std::mem::replace(&mut self.last_status_message, last_status_message)
    }

    // User commands:
    pub fn push_entry(&mut self, user_id: UserId, entry: QueueEntry) {
        self.create_user_queue(user_id).entries.push_back(entry);
    }

    pub fn replace_entry(&mut self, user_id: UserId, entry: QueueEntry) -> Option<QueueEntry> {
        let queue = self.create_user_queue(user_id);
        let removed_entry = queue.entries.pop_back();
        queue.entries.push_back(entry);
        removed_entry
    }

    // Events:
    pub fn next_channel_entry_finished<Delegate: AppModelDelegate>(&mut self, delegate: &Delegate, channel_id: ChannelId) -> Option<QueueEntry> {
        let old_playing_state = std::mem::replace(&mut self.create_channel(channel_id).playing, None);

        // Round-robin to the next user
        let next_user_id = match old_playing_state {
            Some(state) => {
                let last_playing_queue_index = self
                    .queues
                    .iter_mut()
                    .position(|queue| queue.user_id == state.user_id);
                match last_playing_queue_index {
                    Some(last_playing_index) => {
                        // Search queues from after the last active one, back around to it again
                        let queues_iter = self.queues
                            .iter()
                            .skip(last_playing_index + 1)
                            .chain(self.queues.iter().take(last_playing_index + 1));
                        find_first_user_in_channel(queues_iter, delegate, channel_id)
                    }
                    None => find_first_user_in_channel(self.queues.iter(), delegate, channel_id),
                }
            }
            None => find_first_user_in_channel(self.queues.iter(), delegate, channel_id),
        }?;

        let next_queue = self.get_user_queue_mut(next_user_id)?;
        let next_entry = next_queue.entries.pop_front()?;

        // Update channel state to indicate it's playing
        self.create_channel(channel_id).playing = Some(ChannelPlayingState {
            user_id: next_queue.user_id,
            skip_votes: HashSet::new(),
        });

        // Remove any empty queues and channels
        self.queues.retain(|queue| !queue.entries.is_empty());
        self.channels.retain(|_, channel| channel.playing.is_some());

        Some(next_entry)
    }

    pub fn next_channel_entry<Delegate: AppModelDelegate>(&mut self, delegate: &Delegate, channel_id: ChannelId) -> Option<QueueEntry> {
        match self.get_channel_playing_state(channel_id) {
            Some(_) => None,
            None => self.next_channel_entry_finished(delegate, channel_id),
        }
    }

    pub fn vote_for_skip<Delegate: AppModelDelegate>(&mut self, delegate: &Delegate, channel_id: ChannelId, user_id: UserId) -> SkipStatus {
        let skip_votes_required = self.config.skip_votes_required;
        match self.get_channel_playing_state_mut(channel_id) {
            Some(playing_state) => {
                // We can skip immediately if this was the user who's currently playing.
                if user_id == playing_state.user_id {
                    return SkipStatus::OkToSkip;
                }

                // We can skip immediately if the user who played this entry is not in the channel
                // anymore.
                if !delegate.is_user_in_voice_channel(playing_state.user_id, channel_id) {
                    return SkipStatus::OkToSkip;
                }

                // Prevent voting if this user has already skipped
                if playing_state.skip_votes.contains(&user_id) {
                    return SkipStatus::AlreadyVoted;
                }

                // We can skip immediately if we will have the required number of votes
                if playing_state.skip_votes.len() + 1 >= skip_votes_required {
                    return SkipStatus::OkToSkip;
                }

                // Add the vote and indicate more votes are needed
                playing_state.skip_votes.insert(user_id);
                SkipStatus::NeedsMoreVotes(skip_votes_required - playing_state.skip_votes.len())
            },
            None => SkipStatus::NothingPlaying,
        }
    }

    fn get_user_queue_mut(&mut self, user_id: UserId) -> Option<&mut Queue<QueueEntry>> {
        self.queues.iter_mut().find(|queue| queue.user_id == user_id)
    }

    fn create_user_queue(&mut self, user_id: UserId) -> &mut Queue<QueueEntry> {
        // For some reason we need to get the index then lookup instead of using .find() to work
        // around the borrow checker.
        if let Some(existing_queue_index) = self.queues.iter().position(|queue| queue.user_id == user_id) {
            return &mut self.queues[existing_queue_index];
        }

        self.queues.push(Queue {
            user_id,
            entries: VecDeque::new(),
        });
        self.queues.last_mut().unwrap()
    }

    fn create_channel(&mut self, channel_id: ChannelId) -> &mut ChannelModel {
        self.channels.entry(channel_id)
            .or_insert(ChannelModel {
                playing: None
            })
    }

    fn get_channel_playing_state(&self, channel_id: ChannelId) -> Option<&ChannelPlayingState> {
        match self.channels.get(&channel_id) {
            Some(channel) => channel.playing.as_ref(),
            None => None,
        }
    }

    fn get_channel_playing_state_mut(&mut self, channel_id: ChannelId) -> Option<&mut ChannelPlayingState> {
        match self.channels.get_mut(&channel_id) {
            Some(channel) => channel.playing.as_mut(),
            None => None,
        }
    }
}
