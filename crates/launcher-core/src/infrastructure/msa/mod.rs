//! Microsoft (premium) sign-in: the OAuth 2.0 device code flow, then Xbox
//! Live, XSTS and Minecraft Services (`login_with_xbox`), then the
//! entitlement and profile checks. See [`MsaClient`].

mod client;
mod wire;

pub use client::MsaClient;

#[cfg(test)]
mod tests;
