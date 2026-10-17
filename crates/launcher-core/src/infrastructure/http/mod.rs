//! HTTP: the reqwest client and the parallel download manager.

mod client;
mod download;
#[cfg(test)]
mod test_server;

pub use client::ReqwestHttpClient;
pub use download::{DownloadConfig, DownloadManager};
