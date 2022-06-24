use crate::message::{ActionDelegate, ActionUpdater};
use std::any::Any;

pub struct DefaultActionDelegate;

impl ActionDelegate for DefaultActionDelegate {
    fn start(&self, updater: ActionUpdater) -> Box<dyn Any + Send + Sync> {
        Box::new(ActiveActionDelegate {
            updater: Some(updater),
        })
    }
}

struct ActiveActionDelegate {
    updater: Option<ActionUpdater>,
}

impl Drop for ActiveActionDelegate {
    fn drop(&mut self) {
        if let Some(updater) = std::mem::take(&mut self.updater) {
            if !updater.is_response() {
                tokio::task::spawn(updater.delete());
            }
        }
    }
}
