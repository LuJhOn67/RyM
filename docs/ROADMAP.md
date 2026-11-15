# RyM roadmap

Features planned next, in the order suggested. [x] is done (on main, not released yet).

## First
- [ ] **Arabic lyrics translation:** an Arabic line under each line of any song (English, Turkish, Korean...), not only English under Japanese. Decided: only the lyrics sites' own human translations (Musixmatch, Genius Arabic Translations), no machine translation, so a song is never mangled; songs without one show none. Fewer songs get one.
- [x] **Now playing on Discord:** an opt-in setting that posts the playing song to RyM huB's `now-playing` room, with the cover and a "Play in RyM" link, through a relay (Cloudflare Worker, `RyM-discord/hub-worker`) that keeps the webhook. Waits for the relay to be deployed and its address put in `docs/hub.json`.
- [x] **Smart offline:** on Wi-Fi and charging, the phone downloads your top songs by RaM points by itself. Small to medium.

## Strong
- [ ] **Listening party (later):** a room code; whoever joins hears the same song at the same second, from anywhere. Builds on Play along. Big, needs a small relay server.
- [x] **AutoEq for headphones:** pick your headphone model and RyM applies its correction from the open AutoEq database. Builds on the per-device EQ. Medium.

## Nice
- [ ] **Workout mode:** songs at a chosen BPM (the automix already measures it). Medium.
- [x] **Smart playlists:** rules like "high RaM points, not heard this month" or "all by one artist", updating by themselves. Medium.
- [x] **RyM Wrapped:** the year in shareable cards, like the monthly Rewind. Small.
- [x] **Quick Settings tile (phone):** play, pause and like without opening the app. Small.
- [x] **Screen saver mode (PC):** lyrics and cover full screen when idle with music playing. Small.

## Later
- [ ] Chromecast and DLNA.
- [ ] macOS build (later: needs the `workflow` permission on GitHub).
- [ ] Wear OS.

## Upkeep
- [ ] Split `src/bin/slint/main.rs` and `src/bin/gtk/main.rs` by page.
- [ ] Watch YouTube's SABR streaming and Android's developer verification for sideloaded apps.
