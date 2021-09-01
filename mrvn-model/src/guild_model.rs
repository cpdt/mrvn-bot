use serenity::model::prelude::*;
use std::collections::{VecDeque, HashMap};
use crate::app_model_delegate::AppModelDelegate;

fn find_first_user_in_channel<'a, Entry: 'a, Delegate: AppModelDelegate>(mut queues: impl Iterator<Item=&'a Queue<Entry>>, delegate: &Delegate, channel_id: ChannelId) -> Option<UserId> {
    queues
        .find(|queue| delegate.is_user_in_voice_channel(queue.user_id, channel_id))
        .map(|queue| queue.user_id)
}

struct Queue<Entry> {
    user_id: UserId,
    entries: VecDeque<Entry>,
}

struct ChannelPlayingState {
    user_id: UserId,
}

struct ChannelModel {
    playing: Option<ChannelPlayingState>,
}

pub struct GuildModel<QueueEntry> {
    message_channel: Option<ChannelId>,
    queues: Vec<Queue<QueueEntry>>,
    channels: HashMap<ChannelId, ChannelModel>,
}

impl<QueueEntry> GuildModel<QueueEntry> {
    pub fn new() -> Self {
        GuildModel {
            message_channel: None,
            queues: Vec::new(),
            channels: HashMap::new(),
        }
    }

    pub fn message_channel(&self) -> Option<ChannelId> {
        self.message_channel
    }

    // User commands:
    pub fn set_message_channel(&mut self, message_channel: Option<ChannelId>) {
        self.message_channel = message_channel;
    }

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
        });

        // Remove any empty queues
        self.queues.retain(|queue| !queue.entries.is_empty());

        Some(next_entry)
    }

    pub fn next_channel_entry<Delegate: AppModelDelegate>(&mut self, delegate: &Delegate, channel_id: ChannelId) -> Option<QueueEntry> {
        match self.get_channel_playing_state(channel_id) {
            Some(_) => None,
            None => self.next_channel_entry_finished(delegate, channel_id),
        }
    }

    pub fn cleanup(&mut self) {
        // Remove all channels without playback
        self.channels.retain(|_, channel| channel.playing.is_some());
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
}
