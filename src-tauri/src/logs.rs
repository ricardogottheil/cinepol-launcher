//! `LogSink` adapter that forwards game output through a Tauri `Channel`.

use launcher_core::domain::launch::{LogSink, LogStream};
use tauri::ipc::Channel;

use crate::dto::LogLineDto;

pub struct ChannelLogSink {
    channel: Channel<LogLineDto>,
}

impl ChannelLogSink {
    pub fn new(channel: Channel<LogLineDto>) -> Self {
        Self { channel }
    }
}

impl LogSink for ChannelLogSink {
    fn line(&self, stream: LogStream, line: String) {
        // The channel only fails after a page reload; the game keeps running.
        let _ = self.channel.send(LogLineDto::new(stream, line));
    }
}
