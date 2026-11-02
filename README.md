# RyM

An ad-free YouTube Music player, written in Rust, for Windows and Android (Linux coming soon). Your own
account, your library, synced lyrics, and a sound engine with DJ-style song changes.

[العربية](#بالعربي) · [Download](../../releases/latest)

| App | What it is |
|---|---|
| **RymPlayer-GTK** | The window app (GTK 4) for Windows; Linux coming soon |
| **RyM Iced Player** | A second window app (iced), drawn on the graphics card or the processor |
| **RyM** | The same player in the terminal |
| **RyM for Android** | The phone app (Kotlin and Jetpack Compose, Android 10 or newer), in `android/` |

All four share one core: the same account, queue rules, lyrics sources, sound settings and
plugins.

## Features

**Your music**
- Sign in with Google: liked songs, library playlists, likes and listening history stay in sync with YouTube Music.
- **For you**: songs you played lately, a mix made for you (new songs from your recent radio, songs you've never heard, and favourites), then YouTube Music's own shelves. Pages keep loading as you scroll.
- **Up next** follows the playing song: songs like it, with your liked songs mixed in. "Play next" and "Add to queue" go right after the playing song.
- Search, playlists, albums, artists, history, new playlists, and adding any song to a playlist.
- Picks up where you left off, even after a crash: the same song, second and queue.

**Lyrics**
- Synced lyrics from LRCLIB, NetEase and KuGou (asked at once), then Genius and YouTube Music's own. One button per source to switch between them.
- Floating lyrics: an always-on-top line on the PC (click-through or draggable) and a draggable pill on the phone.

**Sound**
- **Smart Automix** (on/off): songs change like a DJ mix. The change starts when a song fades out, the next one comes in on its first sound and on the beat, the outgoing song's bass is filtered out so the two don't clash, and close tempos are matched (within 3%).
- Crossfade (0–10 s, equal-power), skip silence, a 5-band equalizer (Flat, Bass Boost, Vocal, Rock, Pop, Custom) and even loudness.
- Speed and pitch apart (0.5–2.0), with Normal, Nightcore and Slowed.
- Sleep timer (15–60 minutes or the end of the song, fading out over the last 10 seconds).
- **Your recap**: hours listened this month and in all, your top songs and artists.

**Looks**
- Themes, including **RyM YTM**, with its own layout: a side menu with your playlists, big cover shelves, and colours taken from the playing song's cover.
- The song's page: a big cover that zooms in, with the lyrics and Up next beside it.
- English and Arabic (right to left).

**More**
- Plugins from the built-in store: YouTube Video Player & Universal Downloader, Spotify to YTMusic, and SoundCloud.
- Downloads to `Music\RymPlayer`, Discord status (PC), update checks, and VLC as an optional player.

## Download and run

Get the files from [Releases](../../releases/latest):

| File | Run |
|---|---|
| `RyM-GTK-Windows.zip` | Unzip it and run `RymPlayer-GTK.exe` |
| `RyM-Iced-Player.exe` | The Iced Player: run it |
| `RyM.exe` | The terminal app: run it in Windows Terminal |
| `RyM-Android-<version>.apk` | Install it on the phone |

Sign in from the app (Settings, or `g` in the terminal), or browse without an account.

**Optional:** [yt-dlp](https://github.com/yt-dlp/yt-dlp) on your `PATH` (`pip install -U yt-dlp`, with
Node.js) for the best download quality, the SoundCloud and video plugins, and as a backup when
YouTube refuses a stream. WebView2 is needed for the Google sign-in window (built into Windows 11).

## Terminal keys

| Key | Action |
|---|---|
| `↑` `↓` / `j` `k`, `PgUp` `PgDn` | Move |
| `Tab` | Switch between the library and the song list |
| `Enter` | Open / play |
| `Space` | Pause |
| `n` / `p` | Next / previous |
| `←` `→`, `0`–`9` | Seek 5 s, jump to 0–90 % |
| `+` / `-` | Volume |
| `/` | Search |
| `l` | Like / unlike |
| `r` | Radio from the selected song |
| `o` | Song menu |
| `a` | Add to a playlist |
| `Q` | Up next |
| `L` | Lyrics source |
| `[` `]` | Speed and pitch (Normal, Nightcore, Slowed) |
| `z` | Sleep timer |
| `R` | Your recap |
| `h` | History |
| `d` / `D` | Download the song / download all liked songs |
| `s` | Share |
| `A` | Go to the artist |
| `F5` / `u` | Refresh |
| `P` | Plugins |
| `U` | Updates |
| `,` | Settings (sound, Smart Automix, themes, layout, language) |
| `q` | Quit |

The window app: `Ctrl+F` search, `F5` refresh, `Esc` leaves the song's page.

Settings, sign-in data and the song cache live in `%LOCALAPPDATA%\RymPlayer`.

## Build

```sh
cargo build --release --bin RyM                                  # terminal
cargo build --release --bin RymPlayer-GTK --features gtk-ui      # window app (needs GTK 4)
cargo build --release --bin RyM-Iced-Player                      # Iced Player
cd android && ./gradlew assembleRelease                          # phone
```

On Windows, building needs the Visual Studio C++ build tools. CMake compiles libopus; the path in
`.cargo/config.toml` points at the copy that ships with the Build Tools, so change it if yours is
elsewhere.

`cargo test` runs the offline tests; `cargo test -- --ignored --nocapture` also runs the live ones.
On Android: `./gradlew testDebugUnitTest`.

---

## بالعربي

**RyM** مشغّل YouTube Music بلا إعلانات، مكتوب بـ Rust، لويندوز وأندرويد (ولينكس قريباً): حسابك، مكتبتك،
كلمات متزامنة، ومحرك صوت يدمج الأغاني مثل الـ DJ.

| التطبيق | شنو هو |
|---|---|
| **RymPlayer-GTK** | برنامج النوافذ (GTK 4) لويندوز، ولينكس قريباً |
| **RyM Iced Player** | برنامج نوافذ ثاني (iced)، يرسم بكرت الشاشة أو بالمعالج |
| **RyM** | نفس المشغّل بالتيرمنال |
| **RyM للموبايل** | تطبيق أندرويد (أندرويد 10 أو أحدث)، بمجلد `android/` |

### المميزات

**موسيقاك**
- تسجيل دخول بحساب Google: الأغاني المعجبة وقوائم التشغيل والإعجابات وسجل الاستماع كلها متزامنة ويا YouTube Music.
- **لك**: آخر الأغاني اللي سمعتها، مكس معمول إلك (أغاني جديدة من الراديو، أغاني عمرك ما سمعتها، والمفضلة)، وبعدها أقسام YouTube Music. الصفحات تحمّل أكثر كلما تنزل.
- **التالي** يمشي ويا الأغنية اللي تشتغل: أغاني تشبهها ويا أغانيك المعجبة. "تشغيل التالي" و"إضافة لقائمة الانتظار" يجن بعد الأغنية الحالية مباشرة.
- بحث، قوائم تشغيل، ألبومات، فنانين، سجل، قوائم جديدة، وإضافة أي أغنية لقائمة.
- يكمل من وين وقفت حتى لو البرنامج كرش: نفس الأغنية والثانية والقائمة.

**الكلمات**
- كلمات متزامنة من LRCLIB وNetEase وKuGou (يسألهم سوا)، وبعدها Genius وكلمات YouTube Music. زر لكل مصدر حتى تبدّل بينهم.
- كلمات عائمة: سطر فوق كل البرامج بالحاسبة (الضغط يعبر منه أو تسحبه)، وكبسولة تسحبها بالموبايل.

**الصوت**
- **الدمج الذكي (Smart Automix)** تشغيل وإطفاء: الأغاني تتبدل مثل مكس DJ. الدمج يبدي من تخفت الأغنية، والجاية تدخل من أول صوت بيها وعلى الإيقاع، وجهير الطالعة ينشال حتى ما يتصادم الجهيرين، والسرعات القريبة تتطابق (لحد 3%).
- تداخل الأغاني (0–10 ثواني)، تخطي الصمت، معادل صوتي 5 نطاقات (Flat، Bass Boost، Vocal، Rock، Pop، مخصص)، وتوحيد مستوى الصوت.
- السرعة وطبقة الصوت كل وحدة بروحها (0.5–2.0)، ويا Normal وNightcore وSlowed.
- مؤقت النوم (15–60 دقيقة أو نهاية الأغنية، وآخر 10 ثواني يخفت الصوت).
- **ملخصك**: ساعات الاستماع هذا الشهر وبالمجمل، وأكثر الأغاني والفنانين.

**الشكل**
- ثيمات، منها **RyM YTM** بترتيبه الخاص: قائمة جانبية بيها قوائمك، أغلفة كبيرة، وألوان من غلاف الأغنية.
- صفحة الأغنية: غلاف كبير يسوي زوم، وجنبه الكلمات والتالي.
- عربي وإنكليزي (العربي من اليمين لليسار).

**غيرها**
- إضافات من المتجر: مشغّل الفيديو والمحمّل الشامل، Spotify to YTMusic، وSoundCloud.
- تحميل الأغاني لـ `Music\RymPlayer`، حالة Discord (الحاسبة)، فحص التحديثات، وVLC كمشغّل اختياري.

### التنزيل والتشغيل

نزّل من [الإصدارات](../../releases/latest):
- `RyM-GTK-Windows.zip`: فك الضغط وشغّل `RymPlayer-GTK.exe`.
- `RyM-Iced-Player.exe`: الـ Iced Player، شغّله مباشرة.
- `RyM.exe`: التيرمنال.
- `RyM-Android-<النسخة>.apk`: نصّبه بالموبايل.

**اختياري:** [yt-dlp](https://github.com/yt-dlp/yt-dlp) (`pip install -U yt-dlp` ويا Node.js) لأعلى جودة
تحميل، ولإضافتي SoundCloud والفيديو، وكاحتياط إذا يوتيوب رفض الأغنية.

---

## Note · ملاحظة

RyM is an unofficial client and isn't affiliated with Google or YouTube. It uses YouTube Music's
internal API, which can change without notice. Use it with your own account, for personal listening.

RyM برنامج غير رسمي وما إله علاقة بـ Google أو YouTube. يستعمل واجهة YouTube Music الداخلية اللي
ممكن تتغير بأي وقت. استعمله بحسابك، للاستماع الشخصي.

## License

MIT
