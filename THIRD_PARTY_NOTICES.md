# Third-party notices

RyM's own source code is under the MIT License (see `LICENSE`). The apps are built with the
open-source work listed here, each under its own license.

## Which license each app is under

| App | License | Why |
|---|---|---|
| RyM Slint (Windows, Linux) | MIT, with Slint under its Royalty-free License 2.0 | Slint asks for attribution. The app shows it in Settings → About (the `AboutSlint` panel). |
| RyM GTK (Linux) | MIT | GTK and WebKitGTK are LGPL and linked dynamically. |
| RyM terminal | MIT | |
| RyM for Android (the APK) | **GPL-3.0** as a whole | It includes NewPipe Extractor and youtubedl-android, which are GPL-3.0. The full text is in `LICENSES/GPL-3.0.txt`, and the complete source is in this repository. |

## Bundled with the apps

| What | License | Where |
|---|---|---|
| Roboto (font) | Apache License 2.0 | `ui/fonts/LICENSE-roboto.txt` |
| Chathura (font) | SIL Open Font License 1.1 | `ui/fonts/OFL-chathura.txt` |
| Doto (font) | SIL Open Font License 1.1 | `ui/fonts/OFL-doto.txt` |
| Noto Sans JP (font, Linux builds) | SIL Open Font License 1.1 | `OFL.txt` next to it in the AppImage and the tar.gz |
| Solar icon set, by 480 Design | CC BY 4.0 | `ui/icons/NOTICE.txt` |
| yt-dlp's challenge solver (github.com/yt-dlp/ejs) | Unlicense | `assets/ejs/LICENSE` |
| meriyah (inside the solver) | ISC | `assets/ejs/LICENSE` |
| astring (inside the solver) | MIT | `assets/ejs/LICENSE` |

## Rust libraries (PC apps)

Most are under MIT or Apache-2.0. The ones under other licenses:

- **Slint** (`slint`, `i-slint-*`): GPL-3.0-only, or the Slint Royalty-free License 2.0, or the Slint Software License 3.0. RyM uses the royalty-free license and shows the attribution in the app.
- **MPL-2.0** (unmodified; their source is on crates.io): `symphonia` and its codecs, `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`.
- **BSD-3-Clause**: `tiny-skia`, `tiny-skia-path`, `opusic-sys` (libopus), `subtle`, `bindgen`.
- **BSD-2-Clause**: `arrayref`.
- **ISC**: `libloading`, `rustls-webpki`, `untrusted`.
- **Zlib**: `zlib-rs`, `slotmap`, `foldhash`, `const_format`, `konst`.
- **BSL-1.0**: `clipboard-win`, `error-code`.
- **Unicode-3.0**: the ICU4X crates (`icu_*`, `zerovec`, `yoke`, `tinystr`, `litemap`, `writeable`, `zerofrom`, `zerotrie`, `potential_utf`, `fixed_decimal`).

To list every Rust library with its license, run `cargo metadata --format-version 1`.

## Android libraries

| Library | License |
|---|---|
| NewPipe Extractor (TeamNewPipe) | GPL-3.0 |
| youtubedl-android (JunkFood02), which bundles yt-dlp (Unlicense) and Python (PSF License) | GPL-3.0 |
| nanojson (TeamNewPipe fork) | Apache-2.0 |
| AndroidX, Jetpack Compose and Media3 (Google) | Apache-2.0 |
| Kotlin, kotlinx.coroutines and kotlinx.serialization (JetBrains) | Apache-2.0 |
| OkHttp (Square) | Apache-2.0 |
| Coil | Apache-2.0 |

## Data and services

- **SponsorBlock** (sponsor.ajay.app): the "skip non-music parts" segments come from SponsorBlock's database. That data is licensed CC BY-NC-SA 4.0 (sponsor.ajay.app/database). RyM asks with a hashed prefix of the video id and keeps nothing.
- **YouTube Music, YouTube, LRCLIB, the Internet Archive, Last.fm and Discord** are third-party services. RyM isn't affiliated with or endorsed by any of them, and their names are their owners' trademarks.
