use crate::message::ActionUpdater;
use std::any::Any;

pub trait ActionDelegate: 'static + Send + Sync {
    fn start(&self, updater: ActionUpdater) -> Box<dyn Any + Send + Sync>;
}
