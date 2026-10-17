//! `ProgressReporter` adapter that forwards updates through a Tauri `Channel`.

use launcher_core::domain::ports::{ProgressReporter, ProgressUpdate};
use tauri::ipc::Channel;

use crate::dto::ProgressEvent;

pub struct ChannelReporter {
    channel: Channel<ProgressEvent>,
}

impl ChannelReporter {
    pub fn new(channel: Channel<ProgressEvent>) -> Self {
        Self { channel }
    }
}

impl ProgressReporter for ChannelReporter {
    fn report(&self, update: ProgressUpdate) {
        // A send only fails when the webview dropped the channel (page reload);
        // there is nobody left to report to, and the operation keeps going.
        let _ = self.channel.send(ProgressEvent::from(update));
    }
}
