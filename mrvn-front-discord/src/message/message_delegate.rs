use crate::message::ActionUpdater;
use std::any::Any;
use serenity::model::id::{ChannelId, MessageId};

pub trait ActionDelegate: 'static + Send + Sync {
    fn start(&self, updater: ActionUpdater) -> Box<dyn Any + Send + Sync>;
}

pub trait ResponseDelegate: 'static + Send + Sync {
    fn sent(&self, channel_id: ChannelId, message_id: MessageId);
}
