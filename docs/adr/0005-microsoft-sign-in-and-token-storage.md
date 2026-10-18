# ADR 0005: Microsoft sign-in and token storage

- Status: accepted
- Date: 2026-10-17

## Context

Premium players sign in with a Microsoft account. The game needs a Minecraft
Services token, obtained through Xbox Live and XSTS, plus the profile UUID,
name and xuid. Tokens are long-lived credentials (the Microsoft refresh token
lasts about 90 days) and must never reach plain files or logs.

## Decision

- **Flow:** OAuth 2.0 device code flow against the `consumers` tenant with the
  `XboxLive.signin offline_access` scopes, then Xbox Live `user/authenticate`,
  XSTS `xsts/authorize` (relying party `rp://api.minecraftservices.com/`),
  `authentication/login_with_xbox`, the `entitlements/mcstore` ownership check
  and `minecraft/profile`. The device code flow needs no redirect URI and no
  local HTTP server, and works the same on every OS.
- **Ports:** `MicrosoftAuth` (domain) is implemented by `infrastructure/msa`
  over a new `HttpRequester` port. Unlike `HttpClient`, it sends each request
  exactly once and returns every status with its body: device codes and
  refresh tokens are single use, and the error details (`authorization_pending`,
  `XErr`) live in 4xx bodies.
- **Storage:** the refresh token and the cached Minecraft token (with its
  expiry) go to the OS keychain through the `SecretStore` port, implemented
  with the `keyring` crate in `infrastructure/keychain` (macOS Keychain,
  Windows Credential Manager, Linux Secret Service). There is no file
  fallback: an unavailable keychain is an error (`keychain_unavailable`).
  `accounts.json` keeps only the name, profile UUID, xuid and skin link.
- **Refresh:** before every premium launch, `refresh_session` reuses the
  cached Minecraft token while it has more than 5 minutes left; otherwise it
  refreshes with the stored refresh token, stores the rotated one first, and
  logs in to Minecraft Services again. A revoked or missing refresh token is
  `session_expired`, which the UI turns into a re-login prompt.
- **Client id:** `MSA_CLIENT_ID` is read at run time (development) or baked in
  at build time; `src-tauri/build.rs` takes it from the environment or the
  git-ignored `.env`. A build without it hides Microsoft sign-in.
- **Redaction:** besides `Secret`'s redacted `Debug`, a tracing writer in
  `src-tauri/src/redact.rs` masks JWTs, bearer tokens, refresh tokens and the
  values of token-like JSON and form fields before anything is written.

## Consequences

- A new Azure app must be approved by Mojang (https://aka.ms/mce-reviewappid);
  until then `login_with_xbox` answers 403 and the UI shows `app_not_approved`.
- Linux without a Secret Service provider cannot keep premium sessions.
- Skins are fetched by the backend and passed as data URLs, so the CSP keeps
  `img-src` free of remote hosts.
