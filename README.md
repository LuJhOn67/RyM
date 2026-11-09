<div align="center">

# RyM

**An ad-free YouTube Music player for Windows, Linux and Android, written in Rust.**
Your own account and library, synced lyrics, smart suggestions, twelve color themes, and a sound
engine that mixes songs like a DJ.

[**العربية**](#بالعربية) · [**Download**](../../releases/latest) · [Screenshots](#screenshots) · [Build](#build)

![RyM Slint](docs/screenshots/slint-dusk.jpg)

</div>

## The apps

| App | Platform | What it is |
|---|---|---|
| **RyM Slint** | Windows, Linux | The main window app: the playing song beside Up next, similar songs and the artist's songs. Three layouts (Dusk, Mist, Wide) and twelve color themes. |
| **RyM for Android** | Android 10+ | The phone app (Kotlin, Jetpack Compose), in `android/`. |
| **RyM GTK** | Linux | A GTK 4 window app, with the same color themes. |
| **RyM** | Windows, Linux | The same player in the terminal. |

All the apps share one core: the same account, queue rules, suggestions, lyrics sources, sound
settings and plugins.

## Screenshots

### Layouts

| Dusk | Mist | Wide |
|---|---|---|
| ![Dusk](docs/screenshots/slint-dusk.jpg) | ![Mist](docs/screenshots/slint-mist.jpg) | ![Wide](docs/screenshots/slint-wide.jpg) |

| Synced lyrics (Japanese with romaji and English) | Home: suggestions by RaM points |
|---|---|
| ![Lyrics](docs/screenshots/slint-lyrics.jpg) | ![Home](docs/screenshots/slint-home.jpg) |

### The twelve color themes

| OLED Black | Frosted Glass | Tactical Beige |
|---|---|---|
| ![OLED Black](docs/screenshots/theme-oled-black.jpg) | ![Frosted Glass](docs/screenshots/theme-frosted-glass.jpg) | ![Tactical Beige](docs/screenshots/theme-tactical-beige.jpg) |
| **Stellar Neon** | **Dot Matrix** | **Hi-Fi Rack** |
| ![Stellar Neon](docs/screenshots/theme-stellar-neon.jpg) | ![Dot Matrix](docs/screenshots/theme-dot-matrix.jpg) | ![Hi-Fi Rack](docs/screenshots/theme-hi-fi-rack.jpg) |
| **Cyber HUD** | **Lo-Fi Rain** | **Aqua Glass** |
| ![Cyber HUD](docs/screenshots/theme-cyber-hud.jpg) | ![Lo-Fi Rain](docs/screenshots/theme-lo-fi-rain.jpg) | ![Aqua Glass](docs/screenshots/theme-aqua-glass.jpg) |
| **Manga Ink** | **Tactical Olive** | **Remorse** |
| ![Manga Ink](docs/screenshots/theme-manga-ink.jpg) | ![Tactical Olive](docs/screenshots/theme-tactical-olive.jpg) | ![Remorse](docs/screenshots/theme-remorse.jpg) |

### Extras

| Vinyl record | Liquid Aurora |
|---|---|
| ![Vinyl](docs/screenshots/feature-vinyl.jpg) | ![Aurora](docs/screenshots/feature-aurora.jpg) |
| **Wide + Cyber HUD** | **Wide + Hi-Fi Rack (needle meters)** |
| ![Wide HUD](docs/screenshots/feature-wide-hud.jpg) | ![Wide Hi-Fi](docs/screenshots/feature-wide-hifi.jpg) |

### On the phone

| OLED Black | Cyber HUD | Hi-Fi Rack | Dot Matrix |
|---|---|---|---|
| ![](docs/screenshots/phone-player-oled-black.jpg) | ![](docs/screenshots/phone-player-cyber-hud.jpg) | ![](docs/screenshots/phone-player-hi-fi-rack.jpg) | ![](docs/screenshots/phone-player-dot-matrix.jpg) |
| **Lo-Fi Rain** | **Frosted Glass** | **Vinyl record** | **Liquid Aurora** |
| ![](docs/screenshots/phone-player-lo-fi-rain.jpg) | ![](docs/screenshots/phone-player-frosted-glass.jpg) | ![](docs/screenshots/phone-player-vinyl.jpg) | ![](docs/screenshots/phone-player-aurora.jpg) |
| **Settings, Tactical Beige** | **Settings, Manga Ink** | **Search, Aqua Glass** | |
| ![](docs/screenshots/phone-settings-tactical-beige.jpg) | ![](docs/screenshots/phone-settings-manga-ink.jpg) | ![](docs/screenshots/phone-search-aqua-glass.jpg) | |

## Features

**Your music**
- Sign in with Google: liked songs, playlists, likes and history stay in sync with YouTube Music. You can also listen without an account.
- **Songs on this device**: "On this device" in the Library plays your Music folder and any folders you add in Settings. Files opened with RyM Slint play too.
- Search (with suggestions and history), playlists, albums, artists, Spotify and SoundCloud links.
- Picks up where you left off: the same song, second and queue.

**Smart suggestions: RaM points**
- Every song has a kind (songs, Husseini, Quran, nasheeds, talks), and kinds never mix: a song brings songs, a latmiya brings latmiyat.
- Every song earns points when you play it and hear it to the end, and loses them when you skip it in its first seconds. 30% of every change goes to its artist, so new songs by artists you love come up first.
- A song you just played rests for six hours, and points fade a little each day, so the same songs don't top every list.
- "Recommend more" and "Don't recommend" in a song's menu, "Only this kind" in Settings, and buttons to reset all points or only the negative ones.

**Lyrics**
- Synced lyrics from LRCLIB, NetEase and KuGou (asked at once), then Genius and YouTube Music. One button per source.
- Japanese lyrics with romaji and an English translation under each line.
- Floating lyrics: an always-on-top line on the PC, and a draggable pill on the phone.

**Sound**
- **Smart Automix**: songs change like a DJ mix. They start when the old one fades out, on the beat, with the outgoing bass filtered out and close tempos matched.
- Crossfade, skip silence, a 5-band equalizer, even loudness, and speed and pitch apart (Normal, Nightcore, Slowed). A speed button sits next to the heart.
- Repeat all or repeat one, a sleep timer, and an alarm.
- Plays songs as YouTube sends them (Opus first, the best format it has), with no re-encoding.

**Looks**
- Three layouts on the PC (Dusk, Mist, Wide) and **twelve color themes**: OLED Black, Frosted Glass, Tactical Beige, Stellar Neon, Dot Matrix, Hi-Fi Rack, Cyber HUD, Lo-Fi Rain, Aqua Glass, Manga Ink, Tactical Olive and Remorse. The first eleven are on every app; Remorse is on the PC for now.
- **Remorse**, for sadness and guilt: a grey winter night, a lowered voice, an old wound's faded wine, ash falling slowly, and a cover that lost its color.
- **Liquid Aurora**: the cover's four main colors glow behind everything and pulse with the bass.
- **Vinyl record**: the cover turns like a record, faster with Nightcore and slower with Slowed.
- Needle meters (Hi-Fi Rack), dot-matrix digits (Dot Matrix), frosted glass over the cover, and glowing frames in the cover's color.
- Themes travel as `.json` files: export one on the PC and import it on the phone, or the other way round.
- English and Arabic (right to left). On Windows the title bar follows the system's light or dark mode.

**Between your devices**
- **Seamless Music**: open RyM on the phone and it takes the song playing on the PC, at the same second, and the other way round (same Wi-Fi and account).

**More**
- Your listening stats: time this month and in all, your top songs and artists, and what kinds you listen to.
- Share a song as a picture or copy its link. Discord status with "Play this song" and "Get RyM" buttons. Last.fm scrobbling.
- Closing the window keeps the music playing from the tray (or quits, as you choose).
- Plugins: video player and downloader, Spotify to YouTube Music, SoundCloud. Optional playback in VLC.
- Downloads go to `Music\RymPlayer`. Updates are checked from inside the app.

## Download

Get the files from [Releases](../../releases/latest):

| File | For |
|---|---|
| `RyM-Slint.exe` | Windows: run it (one file) |
| `RyM.exe` | Windows: the terminal app (in Windows Terminal) |
| `RyM-Android-<version>.apk` | Android: install it on the phone |
| `RyM-Slint-x86_64.AppImage` | Linux: RyM Slint, the same window app as on Windows (`chmod +x`, then run it) |
| `RyM-x86_64.AppImage` | Linux: RyM GTK (`chmod +x RyM-x86_64.AppImage`, then run it) |
| `RyM-linux-x86_64.tar.gz` | Linux: the same apps unpacked, on the system's GTK 4 (see its README.txt) |

**Linux:** the AppImage carries GTK 4 and the fonts it needs (Japanese included). It needs FUSE 2
(`libfuse2`; without it, run it with `--appimage-extract-and-run`), ALSA (with `pipewire-alsa` or
`pulseaudio-alsa`), and WebKitGTK 4.1 for the Google sign-in window. Media keys work through MPRIS.

**Optional:** [yt-dlp](https://github.com/yt-dlp/yt-dlp) (`pip install -U yt-dlp`, with Node.js) for
the best download quality, the SoundCloud and video plugins, and as a backup when YouTube refuses a
stream. The Google sign-in window on Windows needs WebView2 (built into Windows 11).

Settings, sign-in data and the song cache live in `%LOCALAPPDATA%\RymPlayer` (Windows) and
`~/.local/share/rymplayer` (Linux).

## Keys

**RyM Slint:** `Space` play/pause, `←` `→` skip back/forward (5 s; 10, 15 or 30 in Settings), `↑` `↓` volume, `Ctrl` `+` / `-` / `0` app size, `Ctrl+T` next theme.

**Terminal:**

| Key | Action | Key | Action |
|---|---|---|---|
| `↑` `↓` / `j` `k` | Move | `l` | Like |
| `Tab` | Library / songs | `r` | Radio from the song |
| `Enter` | Open / play | `o` | Song menu |
| `Space` | Pause | `a` | Add to a playlist |
| `n` / `p` | Next / previous | `Q` | Up next |
| `←` `→`, `0`–`9` | Seek | `L` | Lyrics source |
| `+` / `-` | Volume | `[` `]` | Speed (Normal, Nightcore, Slowed) |
| `/` | Search | `z` | Sleep timer |
| `R` | Your recap | `h` | History |
| `d` / `D` | Download song / all liked | `s` | Share |
| `A` | Go to the artist | `F5` / `u` | Refresh |
| `P` | Plugins | `U` | Updates |
| `,` | Settings | `q` | Quit |

## Build

```sh
cargo build --release --bin RyM-Slint --features slint-ui                          # window app
cargo build --release --bin RyM                                                    # terminal
cargo build --release --no-default-features --features gtk-ui --bin RymPlayer-GTK  # GTK (Linux)
cd android && ./gradlew assembleRelease                                            # phone
```

- **Windows** needs the Visual Studio C++ build tools. CMake compiles libopus; `.cargo/config.toml`
  points at the copy in the Build Tools, so change it if yours is elsewhere.
- **Linux:** `packaging/` has the scripts that build on Ubuntu (WSL works) and pack the tar.gz and
  the AppImage.
- **Tests:** `cargo test --lib` (offline), `cargo test -- --ignored --nocapture` (live), and
  `./gradlew testReleaseUnitTest` for the phone.
- **GitHub Actions** builds every push for Windows, Linux and Android, runs the tests and keeps the
  programs on the run's page. A version tag (`v7.1.3`) publishes a release with the files; a tag
  with a dash (`v7.2.0-beta`) publishes a pre-release.
- `RyM-Slint --render out.png [dark|light|wide] [page] ["Theme name"] [vinyl|aurora]` draws the
  window with sample songs, without opening it. The PC screenshots above come from it.

---

<div dir="rtl">

## بالعربية

**RyM** مشغّل لـ YouTube Music بلا إعلانات، يعمل على ويندوز ولينكس وأندرويد، ومكتوب بلغة Rust. يتيح لك
حسابك ومكتبتك، وكلمات أغانٍ متزامنة، واقتراحات ذكية، واثني عشر ثيمًا لونيًا، ومحرّك صوت يدمج الأغاني
كما يفعل منسّقو الموسيقى.

[التنزيل](../../releases/latest) · [الصور](#screenshots)

### التطبيقات

| التطبيق | النظام | الوصف |
|---|---|---|
| **RyM Slint** | ويندوز، لينكس | التطبيق الرئيسي: الأغنية الحالية بجانب قائمة «التالي» والأغاني المشابهة وأغاني الفنان نفسه، بثلاث واجهات (Dusk وMist وWide) واثني عشر ثيمًا لونيًا. |
| **RyM للهاتف** | أندرويد 10 فأحدث | تطبيق الهاتف (Kotlin وJetpack Compose)، في مجلد `android/`. |
| **RyM GTK** | لينكس | تطبيق نوافذ بـ GTK 4، بالثيمات اللونية نفسها. |
| **RyM** | ويندوز، لينكس | المشغّل نفسه داخل الطرفية. |

تشترك التطبيقات كلها في نواة واحدة: الحساب نفسه، وقواعد قائمة التشغيل، والاقتراحات، ومصادر الكلمات،
وإعدادات الصوت، والإضافات.

### المميزات

**موسيقاك**
- تسجيل الدخول بحساب Google، فتبقى الأغاني المعجَب بها وقوائم التشغيل وسجل الاستماع متزامنة مع YouTube Music. ويمكنك الاستماع دون حساب أيضًا.
- **أغاني جهازك**: قسم «على هذا الجهاز» في المكتبة يشغّل مجلد الموسيقى وأي مجلدات تضيفها من الإعدادات، ويشغّل RyM Slint أي ملف صوتي تفتحه به.
- البحث مع الاقتراحات والسجل، وقوائم التشغيل والألبومات والفنانين، وروابط Spotify وSoundCloud.
- يكمل من حيث توقفت: الأغنية نفسها والثانية نفسها وقائمة التشغيل نفسها.

**اقتراحات ذكية: نظام RaM points**
- لكل أغنية نوع (أغانٍ، أو حسيني، أو قرآن، أو أناشيد، أو محاضرات)، ولا تختلط الأنواع أبدًا: الأغنية تجلب أغاني، واللطمية تجلب لطميات.
- تكسب كل أغنية نقاطًا حين تشغّلها وحين تسمعها حتى النهاية، وتخسر نقاطًا حين تتخطاها في ثوانيها الأولى. ويذهب 30% من كل تغيير إلى فنانها، فتظهر أولًا الأغاني الجديدة لفنانيك المفضّلين.
- تستريح الأغنية التي سمعتها للتو ست ساعات، وتخفّ النقاط قليلًا كل يوم، حتى لا تتصدّر الأغاني نفسها كل قائمة.
- خيارا «اقترح أكثر» و«لا تقترح» في قائمة الأغنية، وخيار «هذا النوع فقط» في الإعدادات، وزرّان لتصفير كل النقاط أو السالبة منها فقط.

**الكلمات**
- كلمات متزامنة من LRCLIB وNetEase وKuGou معًا، ثم Genius وYouTube Music، مع زر لكل مصدر.
- الكلمات اليابانية مكتوبة بالحروف اللاتينية (روماجي) ومعها ترجمة إنجليزية تحت كل سطر.
- كلمات عائمة: سطر يبقى فوق كل النوافذ على الحاسوب، وكبسولة قابلة للسحب على الهاتف.

**الصوت**
- **الدمج الذكي (Smart Automix)**: تنتقل الأغاني كأنها مزيج من منسّق موسيقى. يبدأ الانتقال حين تخفت الأغنية، على الإيقاع، مع إزالة الجهير من الأغنية الخارجة ومطابقة السرعات المتقاربة.
- تداخل الأغاني، وتخطي الصمت، ومعادل صوت بخمسة نطاقات، وتوحيد مستوى الصوت، والتحكم بالسرعة وطبقة الصوت كلٌّ على حدة (عادي وNightcore وSlowed)، مع زر للسرعة بجانب زر الإعجاب.
- تكرار القائمة كلها أو أغنية واحدة، ومؤقت للنوم، ومنبّه.
- يشغّل الأغاني كما يرسلها YouTube (بصيغة Opus أولًا، وهي أفضل ما يقدّمه) دون إعادة ترميز.

**المظهر**
- ثلاث واجهات على الحاسوب (Dusk وMist وWide)، و**اثنا عشر ثيمًا لونيًا**: OLED Black وFrosted Glass وTactical Beige وStellar Neon وDot Matrix وHi-Fi Rack وCyber HUD وLo-Fi Rain وAqua Glass وManga Ink وTactical Olive وRemorse. الأحد عشر الأولى متوفرة في كل التطبيقات، وRemorse على الحاسوب حاليًا.
- **Remorse** للحزن والشعور بالذنب: ليلة شتاء رمادية، وصوت خافت، ولون نبيذي باهت كجرح قديم، ورماد يتساقط ببطء، وغلاف فقد ألوانه.
- **الشفق السائل (Liquid Aurora)**: تتوهج ألوان الغلاف الأربعة الرئيسية خلف الواجهة وتنبض مع الجهير.
- **الأسطوانة**: يدور الغلاف كأسطوانة، أسرع مع Nightcore وأبطأ مع Slowed.
- عدّادات بإبرة (Hi-Fi Rack)، وأرقام نقطية (Dot Matrix)، وزجاج ضبابي فوق الغلاف، وإطارات متوهجة بلون الغلاف.
- تنتقل الثيمات كملفات `.json`: صدّر ثيمًا من الحاسوب واستورده في الهاتف، أو العكس.
- العربية والإنجليزية (العربية من اليمين إلى اليسار). وعلى ويندوز يتبع شريط العنوان الوضع الفاتح أو الداكن للنظام.

**بين أجهزتك**
- **الموسيقى المتصلة (Seamless Music)**: افتح RyM على الهاتف فيأخذ الأغنية التي تعمل على الحاسوب من الثانية نفسها، والعكس صحيح (على شبكة Wi-Fi نفسها والحساب نفسه).

**وأكثر**
- إحصاءات استماعك: الوقت هذا الشهر وإجمالًا، وأكثر الأغاني والفنانين، والأنواع التي تسمعها.
- مشاركة الأغنية كصورة أو نسخ رابطها، وحالة Discord مع زرّي «شغّل هذه الأغنية» و«احصل على RyM»، وتسجيل الاستماع في Last.fm.
- إغلاق النافذة يُبقي الموسيقى تعمل من شريط المهام بجانب الساعة، أو يغلق البرنامج، بحسب اختيارك.
- إضافات: مشغّل الفيديو والمحمّل، وتحويل Spotify إلى YouTube Music، وSoundCloud. والتشغيل عبر VLC اختياري.
- تُحفظ التنزيلات في `Music\RymPlayer`، ويُفحص وجود التحديثات من داخل البرنامج.

### التنزيل والتشغيل

نزّل الملفات من [صفحة الإصدارات](../../releases/latest):

| الملف | الاستخدام |
|---|---|
| `RyM-Slint.exe` | ويندوز: شغّله مباشرة (ملف واحد) |
| `RyM.exe` | ويندوز: نسخة الطرفية (في Windows Terminal) |
| `RyM-Android-<الإصدار>.apk` | أندرويد: ثبّته على الهاتف |
| `RyM-x86_64.AppImage` | لينكس: نفّذ `chmod +x RyM-x86_64.AppImage` ثم شغّله |
| `RyM-linux-x86_64.tar.gz` | لينكس: البرامج نفسها دون تغليف، على GTK 4 الموجودة في النظام (اقرأ ملف README.txt بداخله) |

**لينكس:** يحتوي ملف AppImage على GTK 4 والخطوط اللازمة، ومنها الخط الياباني. ويحتاج إلى FUSE 2
(`libfuse2`، وإن لم يكن موجودًا فشغّله بالخيار `--appimage-extract-and-run`)، وإلى ALSA (مع
`pipewire-alsa` أو `pulseaudio-alsa`)، وإلى WebKitGTK 4.1 لنافذة تسجيل الدخول بحساب Google.
وتعمل أزرار الوسائط عبر MPRIS.

**اختياري:** أداة [yt-dlp](https://github.com/yt-dlp/yt-dlp) (`pip install -U yt-dlp` مع Node.js)
لأعلى جودة تنزيل، ولإضافتَي SoundCloud والفيديو، وكبديل حين يرفض YouTube إرسال الأغنية.

</div>

---

## Note · ملاحظة

RyM is an unofficial client and isn't affiliated with Google or YouTube. It uses YouTube Music's
internal API, which can change without notice. Use it with your own account, for personal listening.

<div dir="rtl">

RyM برنامج غير رسمي، ولا علاقة له بـ Google أو YouTube. يستخدم الواجهة البرمجية الداخلية لـ
YouTube Music، وقد تتغير دون إشعار. استخدمه بحسابك الشخصي وللاستماع الشخصي فقط.

</div>

## License

- **RyM's own code:** MIT ([LICENSE](LICENSE)).
- **The Android app:** GPL-3.0 as a whole ([LICENSES/GPL-3.0.txt](LICENSES/GPL-3.0.txt)), because it includes NewPipe Extractor and youtubedl-android.
- **Fonts, icons, Slint, SponsorBlock and the other libraries:** each keeps its own license, listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

<div dir="rtl">

**الرخصة:**
- **كود RyM نفسه:** MIT.
- **تطبيق الأندرويد:** يُوزَّع كاملًا تحت GPL-3.0، لأنه يحتوي NewPipe Extractor وyoutubedl-android.
- **الخطوط والأيقونات وSlint وSponsorBlock وباقي المكتبات:** كل منها برخصته، ومذكورة كلها في THIRD_PARTY_NOTICES.md.

</div>
