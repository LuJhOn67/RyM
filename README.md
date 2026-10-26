# RymPlayer

A fast YouTube Music player for the terminal, written in Rust.

- Your YouTube Music account: liked songs, library playlists, likes and listening history stay in sync
- **For you**: your YouTube Music home feed, with its playlists, albums and mixes
- History, new playlists, and adding any song to your playlists
- Settings screen: 7 themes (including a YouTube Music look), and move or hide the library, lyrics and player bar
- Radio: press `r` on any song, and playback continues with radio when the queue ends
- Best available audio (Opus up to 256 kbps with Premium), no ads
- Synced lyrics from [LRCLIB](https://lrclib.net), with YouTube Music's own lyrics as a fallback
- Keyboard and mouse control, about 30 MB of RAM

## Requirements (Windows 10/11)

| Needed | Install |
|---|---|
| yt-dlp + its YouTube helper | `pip install -U yt-dlp yt-dlp-ejs` |
| Node.js (yt-dlp uses it to unlock YouTube streams) | <https://nodejs.org> |
| WebView2 (only for the Google sign-in window) | built into Windows 11; Windows 10: <https://developer.microsoft.com/microsoft-edge/webview2/> |

## Use

Download `RymPlayer.exe` from [Releases](../../releases) and run it. Press `g` to sign in with Google,
or `s` to browse without an account.

| Key | Action |
|---|---|
| `↑` `↓` / `j` `k`, `PgUp` `PgDn` | Move |
| `Tab` | Switch between the library and the song list |
| `Enter` | Open / play |
| `Space` | Pause |
| `n` / `p` | Next / previous |
| `←` `→` | Seek 5 s |
| `0`–`9` | Jump to 0 %–90 % of the song |
| `+` / `-` | Volume |
| `l` | Like / unlike |
| `r` | Radio from the selected song |
| `h` | History |
| `a` | Add the song to a playlist (or a new one) |
| `N` | New playlist |
| `,` | Settings: theme and layout |
| `/` | Search |
| `g` | Sign in with Google |
| `O` (Shift+o) | Sign out |
| `q` | Quit |

Mouse: click a row to select it, click it again to play; click the progress bar to seek; the wheel scrolls.

Settings are saved in `config.json`. Sign-in data, settings and the song cache live in `%LOCALAPPDATA%\RymPlayer`.

## Build

```sh
cargo build --release
```

Building on Windows needs the Visual Studio C++ build tools. CMake is used to compile libopus; the
path in `.cargo/config.toml` points at the copy that ships with the Build Tools, so adjust it if
yours lives elsewhere.

`cargo test` runs the offline tests; `cargo test -- --ignored --nocapture` also runs the live ones
(search, download, decoding, lyrics, and the signed-in library when you're signed in).

## Note

RymPlayer is an unofficial client and isn't affiliated with Google or YouTube. It uses YouTube
Music's internal API, which can change without notice. Use it with your own account, for personal
listening.

## License

MIT
