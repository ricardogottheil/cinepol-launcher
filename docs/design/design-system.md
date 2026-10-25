# Cinepol Launcher design system

The launcher shares the visual world of the CINEPOL website
(https://cinepol.policarpiers.com/, source in `cinepol-web`). This document
summarizes that system, says where each token comes from, and records how the
launcher translates it to a desktop app. `src/styles/tokens.css` is the code
source of truth. When the website changes, update this file and the tokens
first, then the components.

Source abbreviations used below (paths relative to the `cinepol-web` root):

- **D**: `DESIGN.md` (the website's design system, named "CINEPOL - Fotometro")
- **GC**: `src/styles/global.css`

Reference captures live in `docs/design/screenshots/`: `website/` (live site,
1440px), `launcher/` (this app), and `comparison-home.png` (side by side).

## 1. The idea

The website is a cinematographer's **light meter**: a near-black metal body,
silkscreen labels, engraved rules (D:221-268). The three game worlds are three
**color-temperature readings**: Overworld 5600 K, Nether 2000 K, End 7500 K.
Pages are horizontal bands separated by hairlines. There are no cards.

The launcher is an **Operate** surface: people come to get into the game. The
world contributes type, palette, density and one signature move (the
color-temperature reading); the layout, navigation and controls stay standard
desktop-app patterns.

## 2. Color

### Chassis (constant across all readings)

| Token | Value | Role | Source |
| --- | --- | --- | --- |
| `chassis` | `#0e100e` | App ground, also the window background | GC:16 |
| `panel` | `#1a1d19` | Raised plate, current nav item | GC:17 |
| `panel-deep` | `#080a08` | Groove shadow line | GC:18 |
| `silk` | `#e8e6df` | Primary text (15.30:1 on chassis) | GC:19, D:305 |
| `silk-dim` | `#b4b2ab` | Secondary text | GC:20 |
| `engrave` | `#8a8f86` | Labels, unknown values (5.78:1 on chassis) | GC:21, D:307 |
| `hairline` | `#2a2e29` | Engraved rules | GC:22 |
| `hairline-lit` | `#454a44` | Plate edges, selection, scrollbar thumb | GC:23 |

### Signal

| Token | Value | Source |
| --- | --- | --- |
| `signal` | `#2cb302` | GC:26 |
| `signal-lit` | `#68ca4a` | GC:27 |
| `on-signal` | `#0e100e` (6.87:1; white would be 2.78:1) | GC:28 |

**Palette law** (D:274-283): on the website `signal` appears on exactly two
things: the primary signal control and the live-status lamp. Never a heading,
gradient or decoration. Text on signal is always `on-signal`, never white.

**Launcher divergence** (2026-10-25, players found the app too grey): the
launcher also uses `signal` for state, never decoration. It marks where the
player is and what is moving: the current sidebar item (a 3px bar), the
selected tab's underline, the keyboard focus ring (6.87:1 on `chassis`), the
fill of progress meters and the lamp of a success toast. `signal` still fills
only one control per screen, the primary one; headings, plain borders, text and
backgrounds stay chassis.

**Launcher translation:** the website's signal control is "Copy IP". In the
launcher it is **"Play on CINEPOL"**, the primary action. "Copy IP" becomes a
secondary plate button. The website applies `signal-lit` on hover
(CopyAddress.astro:41) despite D's "never a hover tint"; the launcher follows the
shipped code.

### World readings

Each reading has a ground plus inks. Ratios are measured on the reading's own
ground (GC:30-34). Worlds are separated by hue, not luminance; any new ground
needs at least 60 degrees of hue separation (D:366-371).

| Reading | Ground | Text-safe inks | Field-only | Source |
| --- | --- | --- | --- | --- |
| Overworld, 5600 K | `ow-ground #1f262c` | `ow-sky #b7cbdd`, `ow-cloud #c6cbd5`, `ow-blossom #b88898` | `ow-foliage #72915b` (large text), `ow-oak #8e6f50` (large text), `ow-roof #9b5252`, `ow-spruce #394030` | GC:36-44 |
| Nether, 2000 K | `nt-ground #2e130d` | `nt-lava #de9133`, `nt-beacon #8fe3e3` | `nt-crimson #942a12`, `nt-basalt #5a2416` | GC:46-51 |
| End, 7500 K | `end-ground #281e2c` | `end-glow #e4d8e4`, `end-stone #be9f9f` | `end-plum #6c5059`, `end-violet #6d4a86`, `end-deep #4f3759` | GC:53-59 |

Resolved source conflicts (the stricter reading wins):

- `ow-foliage` (4.31:1) is "text-safe" in D:25 and GC:40, but "large text and
  field" in D:329. Launcher: large text and field only.
- `ow-roof` (2.71:1) is "large text and field" in GC:42, "field only" in D:332.
  Launcher: field only.

Ores (`ore-coal`, `ore-emerald`, `ore-gold`, `ore-lapis`, `ore-diamond`,
`ore-amethyst`, GC:65-70) are ported for future data series.

### Semantic

`live #2cb302` and `stale #de9133` (GC:73-74). **There is no error red**
(D:381): a failed server status reads in `engrave` with plain words, never red.
Destructive confirmations (deleting an instance) use the launcher-only
`hazard` pair: `hazard #de9133` ink on `hazard-ground #2e130d`, the Nether's
lava on its own ground (6.75:1, text-safe). It is the Nether reading, not a
new red, so the "no error red" rule holds. The dialog sits on `scrim`
(`panel-deep` at 80%). The destructive button is the only `hazard` element on
screen; the safe choice (Cancel) takes the initial focus. Crash reports still
have no token; propose one here before using it.

## 3. Typography

Self-hosted variable fonts (SIL OFL 1.1, licenses in `src/assets/brand/fonts/`),
the same packages as the website (`@fontsource-variable/*` 5.3.0):

| Family | Token | Role | Source |
| --- | --- | --- | --- |
| Archivo Variable | `font-display` | Display, plates, navigation, controls | GC:77 |
| Martian Mono Variable | `font-readout` | Every number, addresses, versions | GC:78 |
| Schibsted Grotesk Variable | `font-body` | Body copy (1rem, line-height 1.55) | GC:79, GC:132-140 |

Scale (utilities in `tokens.css`):

| Utility | Settings | Source |
| --- | --- | --- |
| `display-xl` | Archivo wdth 125, 700, uppercase, 0.95, -0.02em | GC:204-221 |
| `display-lg` | Archivo wdth 125, 700, uppercase, 1.0, -0.01em | GC:223-236 |
| `display-md` | Archivo wdth 112, 600, uppercase, 1.5rem, 1.1 | GC:238-245 |
| `plate` | Archivo 600, uppercase, 11px (hard floor), 0.18em | GC:252-259 |
| `plate-lg` | Archivo 600, uppercase, 12px, 0.14em | GC:261-268 |
| `readout-xl` / `-md` / `-sm` | Martian Mono, tabular, 2rem / 1.25rem / 0.8125rem | GC:270-295 |
| `control-label` | Archivo 600, uppercase, 14px, 0.1em | GC:297-304 |
| `nav-label` | Archivo 500, uppercase, 13px, 0.08em | GC:306-313 |
| `measure` | `max-width: 68ch` | GC:322-324 |

Launcher change: `display-xl` and `display-lg` use a single `clamp()` instead of
the website's `lg` variant, because the app window has no mobile breakpoint.

Principles (D:417-446): every number is a readout; uppercase is structural, not
emphasis; display type is flush left; **no kicker labels above headings**;
numbered steps only where order is the information; mixed sizes in a row align
on the baseline.

## 4. Space, shape, depth

- Spacing: `band 72px`, `band-lg 112px`, `gutter 24px`, `gutter-lg 32px`
  (GC:85-95). D's scale: 1, 4, 8, 12, 16, 24, 32, 48, 72, 112 (D:160-170).
- Radii: `plate 2px`, `control 3px`; everything else is square. **No pills**;
  the only circle is the status lamp (GC:81-83, D:564).
- Borders: 1px `hairline` for rules, 1px `hairline-lit` for plate edges.
  `groove` is the only inset device (GC:316-319).
- Depth: **no shadows, no blur, no glass, no glow**, no gradient except a photo
  scrim (D:550-558).
- Minimum target size: 44px (D:1007-1015), token `--spacing-control`.

Launcher chrome tokens (not on the website): `titlebar 44px` (matches the site's
section strip), `sidebar 248px`.

## 5. Motion

Curves (GC:346-351): `ease-shutter cubic-bezier(0.85, 0, 0.15, 1)` for moving
parts, `ease-settle cubic-bezier(0.16, 1, 0.3, 1)` for things that settle.
Ported in `src/styles/motion.css`:

| Name | Use in the launcher | Source |
| --- | --- | --- |
| `enter-wipe` (620ms, settle) | Home headline, once | GC:356-370 |
| `enter-rule` (720ms, 270ms delay) | The rule under the lead | GC:373-386 |
| `enter-control` (520ms, 520ms delay) | The play control arrives last | GC:389-403 |
| `lamp` (2600ms) | Live dot, only while the server answers | GC:408-422 |
| `control-flash` (420ms, shutter) | Signal control acknowledges a press | GC:474-488 |
| underline (260ms, settle) | `[data-underline]` links | GC:448-471 |
| `toast-in` (280ms, settle) | A toast rises 12px into place | Launcher only |
| hover colors (160ms, settle) | Plates, nav items | GC:567-645 |

`prefers-reduced-motion: reduce` lands every final state with no animation.
Nothing animates off screen and nothing listens to scroll (D:876-889).

## 6. Components

All live in `src/components/ui/` and are previewed on the dev-only route
`/design` ("Design kit").

| Component | Website origin | Notes |
| --- | --- | --- |
| `SignalControl` | Copy-IP control, CopyAddress.astro:28-83 | 56px full / 44px compact, readout + control label, baseline-aligned |
| `Button` | Lightbox and BackToTop plates | Secondary actions; never signal |
| `IconButton` | Lightbox controls, Lightbox.astro:21 | 44px square plate, accessible name required |
| `Readout` | Readout.astro:22-34 | Unit plate above value; unknown value is an em dash, never 0 |
| `StatusLamp` | Live status, Hero.astro:25-53 | Rendered only when online |
| `EquipmentPlate` | Equipment plate, D:195-201, D:669-680 | The cinema caption: subject + color temperature |
| `WorldFrame` | Worlds gallery, WorldGallery.astro:46-89 | 16:9 capture with its plate |
| `SpecSheet` | World readings ("label -> value") | Instance details: version, loader, Java, memory |
| `PlayerHead` | Launcher only | A profile's face from its skin (face + hat layer, pixelated); initial on a plate without a skin |
| `CapeSwatch` | Launcher only | A cape's front (10x16 at 1,1 of its texture, pixelated); an empty plate without a texture |
| `Meter` | (launcher only) | Progress as a reading: hairline track, signal fill, tabular count |
| `StepList` | How to join, HowToJoin.astro:43-58 | Numbered steps where order matters (Microsoft sign-in) |
| `BrandGlyph` | src/icons/brandGlyphs.ts | Twitch and Discord only, simple-icons (CC0) |
| `Toaster` | Launcher only | Passing notices bottom right, mounted once in the shell; see below |
| `Wordmark` | `src/assets/brand/logo-wordmark.png` | The only logo rendered; never `logo.png` (it carries Mojang's badge, D:997) |

**Toasts** (`toast()` in `src/components/ui/toastStore.ts`): a `panel` plate
with a `hairline-lit` edge, a small lamp (`signal` for something the player
did, `engrave` for what happened instead), the message in `silk` and a close
button. One leaves after 5 s unless the pointer or focus rests on it; at most
three show. They carry only what passes: a cancel, "Saved", a finished update,
export or mod install. Errors, crashes and anything the player must act on
stay inline where they happened. A cancel is the player's own choice: it never
leaves an error behind.

App shell (`src/app/shell/`):

- **Title bar**: 44px, chassis, hairline bottom edge, whole bar is a drag
  region. Windows and Linux draw minimize / maximize / close as square plates;
  macOS keeps the native traffic lights (overlay title bar,
  `src-tauri/tauri.macos.conf.json`) and the wordmark clears them.
- **Sidebar**: text-only section rail, like the website's section strip (no
  decorative icons, D:239-268), grouped by what the player is doing: Game
  (Home, Instances, Servers, Modpacks), Player (Profile), Launcher (Settings). Group
  names are `plate` in `engrave`; current item: `silk` on `panel` with a 3px
  `signal` bar on its left edge. Community
  links (Twitch, Discord) sit above the profile switcher at the foot.
- **Profile switcher**: the active profile (head, name, kind plate) at the
  foot of the sidebar. It opens a menu on `panel` with a `hairline-lit` edge
  that lists online (Microsoft) and offline profiles in separate groups, plus
  "View profile" and "Add profile". Heads come from the skin's face and hat
  layer (`PlayerHead`); profiles without a skin show their initial, never one
  of the game's default players.
- **Profile page**: the head and name, the 3D skin with the worn cape, the
  label -> value sheet, then for premium profiles "Change skin" and "Cape".
  The figure rests (arms down, cape out, a slow breath) at a front
  three-quarter view and never turns by itself; dragging turns the figure
  (the camera stays put), and two turn buttons under it do the same from the
  keyboard. Under reduced
  motion it stands still and the buttons turn it at once. No zoom or pan.
  "Change skin" picks the model first (Classic / Slim as two plates, each
  with a small figure whose arms are 4 or 3 pixels wide, as on
  minecraft.net), then the PNG; both preview on the figure before one
  "Apply", and a model picked alone already shows on the current skin. The capes are toggle buttons that only preview on the figure; the
  worn one is marked "Worn", and "Apply cape" wears the chosen one with one
  request. "Delete profile" reads in `hazard` and asks in the destructive
  `ConfirmDialog`, like deleting an instance.
- **Modpacks**: tabs Explore / Installed / Import (`nav-label`, the current
  tab underlined in `signal`). Explore puts the filters on top and the packs as
  cards (icon, title as a stretched link, two-line description, loader
  plates, downloads readout) in an auto-fill grid; a card opens the pack's own
  page (`/modpacks/<catalog>/<project>`) with its versions. Installed reuses
  the instance rows; Import groups the file pickers and packwiz. The tab, the
  catalog and the search survive opening a pack.
- **Instances**: the list keeps each row to its spec sheet (name as a link,
  version, loader, playtime), Play and one status line; rows are separated by
  a single hairline, and empty status lines take no room. The name opens the
  instance's own page (`/instances/<id>`): a back link, the name as `display-lg`,
  the spec sheet and the actions (Open folder, Update, Export, Install, Play),
  then tabs Servers / Mods / Settings / Log styled like the Modpacks tabs.
  Starting the game switches to Log. Deleting the instance returns to the list.
- **Mods tab**: the installed jars as rule-separated rows (title, version
  readout, its catalog, why it is there: "Needed by ..." or "Not on Modrinth", an "Off"
  plate when disabled; then Update to X, Turn on/off, Remove as plate
  buttons), with a Mods count readout, "Check for updates" (on request; mods
  it could not check are named in `stale`) and "Add mods". "Add mods" shows
  the filter as a readout ("Compatible with 1.21.1 · Fabric"), never as a
  choice, a Modrinth / CurseForge select before the search field (only when
  CurseForge is configured, as on the modpacks page), and results as rows
  (icon, title, two-line description, downloads readout, Install / Installed
  / Not compatible, or "Open page" for a mod only downloadable by hand from
  CurseForge). Installed rows name their catalog after the version. Install first opens a
  `ConfirmDialog` listing every mod it writes with "for <mod>" on
  dependencies; a refusal stays on the row in `stale`, naming the other mod.
  Remove uses the destructive dialog, with a checkbox (checked) for the
  dependencies nothing else needs and a `hazard` line when other mods need
  it. The running install or update shows its stage, meter and Cancel under
  the page header, so it survives switching tabs. Vanilla instances show one
  line saying a loader is needed.
- **Servers**: CINEPOL pinned first on a `panel` plate with its live status
  and the page's only signal control (Play); then "Your servers" as rows
  (name, address readout, status lamp, players, version, latency, the
  instance select, then Play, Edit and Remove as plate buttons); then the add
  form. Play without an instance focuses the select and says why.
- **Settings**: only About, the version and the legal disclaimer. The server
  configuration URL is not offered to players.
- **Home**: a launcher view in the website's style, not the website's content.
  Quick play on CINEPOL (server, recommended version, active account as a spec
  sheet; the signal control "Play"), the CINEPOL banner with its equipment plate
  and the live readouts, then the instances band with its actions. Each
  instance row names it (a link to its own page), reads "loader · version ·
  playtime" ("Never played" before the first game) and has its own Play; the
  row lights to `panel` on hover. Marketing
  copy from the website (headlines, pitch, "Copy IP") does not belong here.

## 7. Iconography and imagery

- Icons only where they do work (D:239-268). The website's own glyphs are 12x12
  texel grids with `crispEdges`. Window controls follow that: 10x10, 1px,
  `crispEdges`. Never emoji or Unicode glyphs as icons.
- Imagery: real server captures only, 16:9, no Minecraft UI (hotbar, hearts,
  chat, F3). Never set type directly on a bare screenshot: captions go on an
  equipment plate. Assets: `src/assets/worlds/` (see `SOURCE.md` there).
- Brand assets (`src/assets/brand/`, `src/assets/worlds/`) are not covered by
  the GPL (see README).

## 8. The world readings in the launcher

The website uses the three readings for its world bands and gallery captions.
The launcher uses them as **theming for areas**, never as decoration
everywhere:

- World captures carry their color temperature on an equipment plate.
- Proposed for later milestones (to confirm when built): instance rows take the
  reading of their loader family (Vanilla: Overworld, Fabric/Quilt: End,
  Forge/NeoForge: Nether) as a ground band; loading states for long installs
  use the reading's ground with its text-safe ink.

Note: the live website still shows the color-temperature captions and a
separate skin section, but the current website source removed both on
2026-10-12 (D:711, D:472). The launcher keeps the caption motif on purpose,
because the launcher brief asks for it.

## 9. Browser surfaces

Themed, never browser defaults (GC:144-190): selection `silk` on `hairline-lit`;
caret `signal`; 10px scrollbar with `hairline-lit` thumb (`engrave` on hover);
`::marker` in `engrave`; focus ring 2px `signal` with 2px offset; link underline
offset 0.2em, 1px thick. Window chrome is not text-selectable.

## 10. Copy

Spanish (`es`, the default locale) follows the website: neutral Spanish from
Spain, short functional sentences, second person singular, no cinema wordplay
(PRODUCT.md:307-309). English (`en`) is the source locale. Status copy never
blames the player: an unreadable status says the address still works.

## 11. Do and don't

Do: signal fills only the primary control; otherwise it marks state (lamp,
current item, focus, progress); dark ink on green; every
figure as a readout with a unit; separate bands with rules; an em dash for
unknown values; field-only colors only as fields.

Don't: cards; pills; shadows, blur, glass or glow; kickers above headings;
centered display headings; white text on green; opacity on text; red for a
failed status; invented world colors; type on bare screenshots; browser-default
selection, caret, scrollbar or focus; rendering `logo.png`.

Known divergence to resolve later: the launcher brief mentions "instance cards".
The design system bans cards, so instances will be rule-separated rows with a
reading band, not cards.
