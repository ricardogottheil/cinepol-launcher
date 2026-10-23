# ADR 0011: Skin and cape changes

- Status: accepted
- Date: 2026-10-23

## Context

Players with a Microsoft account want to change their skin and pick a cape
from the launcher's profile page. Minecraft Services takes a skin as a
multipart upload (`POST /minecraft/profile/skins`, fields `variant` and
`file`) and a cape by id (`PUT` or `DELETE /minecraft/profile/capes/active`),
with the player's Minecraft token. Mojang's documentation warns that many
rate-limited skin uploads can lead to an account suspension.

## Decision

- **One request per action.** Skin uploads and cape changes go through the
  single-shot `HttpRequester` port (never retried), and the UI mutations do
  not retry either. A refused or rate-limited change is reported
  (`skin_rejected`, `cape_not_owned`, `too_many_changes`); the player decides
  whether to try again. Once Mojang applied an upload it is reported as done,
  even when its answer is unreadable or storing the new skin fails, so the
  player is never led to upload again.
- **Checked before upload.** `SkinPng` accepts only a PNG whose IHDR header is
  64x64 or 64x32 and at most 256 KiB. The UI runs the same check on the picked
  file, so a wrong file fails at once and never counts against the limit.
- **Tokens.** Changes use `MicrosoftSessions::session_token`, the same refresh
  path as launching; an expired session asks the player to sign in again.
- **Storage.** After an upload, the skin Mojang reports replaces the skin link
  of the account as stored after the upload (re-read, so a concurrent sign-in
  is not overwritten) in `accounts.json` (same format); an answer without a
  skin keeps the stored one. Capes are never stored:
  the profile page reads them live from `GET /minecraft/profile`, with their
  textures, and a cape whose texture fails to download is still listed.
- **Transport.** `HttpRequester` gained `PUT`, `DELETE` and a
  `multipart/form-data` body, encoded by the launcher (RFC 7578) instead of
  enabling reqwest's `multipart` feature. Field names, file names and media
  types with quotes or line breaks are refused before sending.
- **Offline profiles** cannot change skins or capes; the page says so.

## Consequences

- The launcher never uploads automatically or in a loop, at the cost of the
  player retrying by hand after a transient failure.
- The cape list costs one profile request and one texture download per cape
  each time it is shown (cached for five minutes in the UI).
- Resetting to the default skin is not offered yet.
