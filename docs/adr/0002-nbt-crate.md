# ADR 0002: NBT crate for servers.dat

- Status: accepted
- Date: 2026-10-17

## Context

Each instance keeps a `servers.dat` (uncompressed NBT). We must list, add, edit,
remove and reorder servers while **preserving unknown tags** written by the game
or by mods. The candidates were `valence_nbt` and `fastnbt` (`hematite_nbt` is
unmaintained).

## Decision

Use **`fastnbt`** (2.6.3).

- Maintained: latest release August 2026. `valence_nbt` 0.8.0 dates from
  October 2023 and follows the Valence server's needs rather than a stable API.
- serde integration: typed structs for the known fields (`name`, `ip`, `icon`,
  `acceptTextures`, `hidden`), plus `fastnbt::Value` for a lossless dynamic view.
- Reads and writes uncompressed NBT directly (`from_bytes` / `to_bytes`).

## How unknown tags are preserved

Each server entry is parsed as a `fastnbt::Value::Compound`. Known fields are
read from and written into that same compound, so every other tag (and the
root's other tags) survives a round trip. The adapter lives in
`infrastructure/nbt/`, behind a `ServerListStore` port.

## Consequences

- A fixture `servers.dat` with extra tags must round-trip byte-for-byte equal in
  content (tag order inside compounds is not semantically meaningful in NBT).
- If round-trip fidelity proves insufficient, revisit this ADR before Milestone 13.
