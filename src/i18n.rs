//! The apps' words in English or Arabic. Text is written in English in the code; [`tr`] gives
//! the Arabic when Arabic is on (Settings > Language), and the English otherwise.

use std::sync::atomic::{AtomicBool, Ordering};

static ARABIC: AtomicBool = AtomicBool::new(false);

pub fn set_arabic(on: bool) {
    ARABIC.store(on, Ordering::Relaxed);
}

pub fn is_arabic() -> bool {
    ARABIC.load(Ordering::Relaxed)
}

/// The text in the app's language (English when a text has no Arabic yet).
pub fn tr(en: &'static str) -> &'static str {
    if is_arabic() { arabic(en).unwrap_or(en) } else { en }
}

/// Every Arabic text, by its English.
fn arabic(en: &str) -> Option<&'static str> {
    Some(match en {
        // ---- pages and navigation
        "Home" => "الرئيسية",
        "For you" => "لك",
        "★ For you" => "★ لك",
        "Search" => "بحث",
        "SEARCH" => "بحث",
        "Library" => "المكتبة",
        "History" => "السجل",
        "HISTORY" => "السجل",
        "⏱ History" => "⏱ السجل",
        "Settings" => "الإعدادات",
        "⚙ Settings ," => "⚙ الإعدادات ,",
        "Liked" => "المعجبة",
        "Liked songs" => "الأغاني المعجبة",
        "Liked music" => "الأغاني المعجبة",
        "Liked Music" => "الأغاني المعجبة",
        "On this device" => "على هذا الجهاز",
        "New playlist" => "قائمة جديدة",
        "＋ New playlist" => "＋ قائمة جديدة",
        "Now playing" => "يُشغَّل الآن",
        "Up next" => "التالي",
        "UP NEXT" => "التالي",
        "Lyrics" => "الكلمات",
        "LYRICS" => "الكلمات",
        "New for you" => "جديد لك",
        "Account" => "الحساب",
        "Look" => "المظهر",
        // ---- kinds and columns
        "Playlist" => "قائمة تشغيل",
        "PLAYLIST" => "قائمة تشغيل",
        "Album" => "ألبوم",
        "ALBUM" => "ألبوم",
        "Mix" => "مزيج",
        "MIX" => "مزيج",
        "Song" => "أغنية",
        "ARTIST" => "فنان",
        "Artist" => "الفنان",
        "Title" => "العنوان",
        "Time" => "المدة",
        "When" => "متى",
        "Section" => "القسم",
        "Auto playlist" => "قائمة تلقائية",
        "songs" => "أغنية",
        "columns" => "عمود",
        "at once" => "معاً",
        // ---- actions
        "Sign in" => "تسجيل الدخول",
        "Sign in with Google" => "تسجيل الدخول بحساب Google",
        "Sign in with Google (opens a sign-in window)" => "تسجيل الدخول بحساب Google (تفتح نافذة)",
        "Sign out" => "تسجيل الخروج",
        "Paste a browser cookie" => "لصق كوكي من المتصفح",
        "Continue without an account" => "المتابعة بدون حساب",
        "Songs on this device (no account needed)" => "أغاني هذا الجهاز (بدون حساب)",
        "Quit" => "خروج",
        "Refresh" => "تحديث",
        "Refreshing..." => "يتم التحديث...",
        "Refreshing…" => "يتم التحديث…",
        "New songs on top (F5)" => "أغانٍ جديدة في الأعلى (F5)",
        "Refresh: new songs on top (F5)" => "تحديث: أغانٍ جديدة في الأعلى (F5)",
        "Play all" => "تشغيل الكل",
        "Play next" => "تشغيل بعد هذه",
        "Add to queue" => "إضافة إلى قائمة الانتظار",
        "Remove from queue" => "إزالة من قائمة الانتظار",
        "Dismiss queue" => "مسح قائمة الانتظار",
        "Start radio" => "تشغيل راديو",
        "Start mix" => "تشغيل مزيج",
        "Add to liked songs" => "إضافة إلى المعجبة",
        "Remove from liked songs" => "إزالة من المعجبة",
        "Download" => "تنزيل",
        "Download all" => "تنزيل الكل",
        "How many (empty = all)" => "كم أغنية (فارغ = الكل)",
        "Downloading songs" => "جارٍ تنزيل الأغاني",
        "saved" => "حُفظت",
        "already there" => "موجودة مسبقاً",
        "failed" => "فشلت",
        "Finding the artist..." => "جارٍ إيجاد الفنان...",
        "Artist not found" => "لم يتم العثور على الفنان",
        "Version" => "الإصدار",
        "Updates" => "التحديثات",
        "Automatic" => "تلقائي",
        "Enter download  Esc later" => "Enter تنزيل  Esc لاحقاً",
        "Enter install / remove  Esc close" => "Enter تثبيت / إزالة  Esc إغلاق",
        "Paste a link from YouTube or any site (Enter / Esc)" => "الصق رابطاً من يوتيوب أو أي موقع (Enter / Esc)",
        "Install the video plugin first (P)" => "ثبّت إضافة الفيديو أولاً (P)",
        "updates" => "التحديثات",
        "plugins" => "الإضافات",
        "video" => "فيديو",
        "Check for updates automatically" => "التحقق من التحديثات تلقائياً",
        "Every 12 hours, and when the app opens" => "كل 12 ساعة، وعند فتح البرنامج",
        "Beta updates" => "تحديثات بيتا",
        "Test versions (pre-releases) count as updates too" => "النسخ التجريبية (Pre-release) تُعدّ تحديثات أيضاً",
        "Check for updates" => "التحقق من وجود تحديثات",
        "Checking for updates..." => "جارٍ التحقق من التحديثات...",
        "You have the latest version" => "عندك أحدث إصدار",
        "Couldn't check for updates" => "تعذّر التحقق من التحديثات",
        "New update" => "تحديث جديد",
        "New beta" => "نسخة بيتا جديدة",
        "What's new" => "الجديد",
        "Later" => "لاحقاً",
        "You have" => "عندك",
        "Plugins" => "الإضافات",
        "Plugin store" => "متجر الإضافات",
        "Add features to RyM; without them it stays a music player" => "أضف ميزات لـ RyM؛ بدونها يبقى مشغّل موسيقى",
        "Open" => "فتح",
        "Install" => "تثبيت",
        "Update" => "تحديث",
        "Installed" => "مثبّتة",
        "Loading the store..." => "جارٍ تحميل المتجر...",
        "Plugin installed" => "تم تثبيت الإضافة",
        "Plugin removed" => "تمت إزالة الإضافة",
        "Plugin updates" => "تحديثات الإضافات",
        "Watch video" => "مشاهدة الفيديو",
        "Download video" => "تنزيل الفيديو",
        "Download from a link" => "تنزيل من رابط",
        "Paste a link from YouTube or any site" => "الصق رابطاً من يوتيوب أو أي موقع",
        "Video" => "فيديو",
        "Audio only" => "صوت فقط",
        "Opening the video..." => "جارٍ فتح الفيديو...",
        "VLC isn't installed: opening YouTube in the browser" => "VLC غير مثبت: فتح يوتيوب في المتصفح",
        "Can't open the video" => "تعذّر فتح الفيديو",
        "song menu" => "قائمة الأغنية",
        "artist" => "الفنان",
        "download all" => "تنزيل الكل",
        "Enter choose  Esc close" => "Enter اختيار  Esc إغلاق",
        "Download how many liked songs? (empty = all, Enter / Esc)" => "كم أغنية معجبة تُنزَّل؟ (فارغ = الكل، Enter / Esc)",
        "Open the Liked songs page first" => "افتح صفحة الأغاني المعجبة أولاً",
        "Go to album" => "الذهاب إلى الألبوم",
        "Go to artist" => "الذهاب إلى الفنان",
        "Share" => "مشاركة",
        "Save to playlist" => "حفظ في قائمة",
        "Add to playlist" => "إضافة إلى قائمة",
        "Make an empty one" => "إنشاء قائمة فارغة",
        "Cancel" => "إلغاء",
        "Create" => "إنشاء",
        "Close" => "إغلاق",
        "Like" => "إعجاب",
        "Previous" => "السابق",
        "Next" => "التالي",
        "Play / pause" => "تشغيل / إيقاف مؤقت",
        // ---- key hints (terminal)
        "pause" => "إيقاف",
        "next/prev" => "التالي/السابق",
        "seek" => "تقديم",
        "like" => "إعجاب",
        "radio" => "راديو",
        "download" => "تنزيل",
        "search" => "بحث",
        "history" => "السجل",
        "add to playlist" => "إضافة لقائمة",
        "switch" => "تنقل",
        "settings" => "الإعدادات",
        "refresh" => "تحديث",
        "quit" => "خروج",
        "vol" => "الصوت",
        // ---- states and messages
        "Loading..." => "جارٍ التحميل...",
        "Loading…" => "جارٍ التحميل…",
        "Loading your feed…" => "جارٍ تحميل صفحتك…",
        "Nothing here yet" => "لا يوجد شيء هنا بعد",
        "Nothing playing" => "لا شيء يُشغَّل",
        "Pick something to play" => "اختر شيئاً لتشغيله",
        "Type in the search box above" => "اكتب في مربع البحث في الأعلى",
        "Press / to search" => "اضغط / للبحث",
        "No songs here" => "لا توجد أغانٍ هنا",
        "Search songs, artists..." => "ابحث عن أغانٍ وفنانين...",
        "Search songs, albums, artists" => "ابحث عن أغانٍ وألبومات وفنانين",
        "YouTube Music in your terminal" => "يوتيوب ميوزك في الطرفية",
        "YouTube Music, your way" => "يوتيوب ميوزك على طريقتك",
        "Signing in..." => "جارٍ تسجيل الدخول...",
        "Connecting..." => "جارٍ الاتصال...",
        "Signed in" => "تم تسجيل الدخول",
        "Finish signing in in the Google window..." => "أكمل تسجيل الدخول في نافذة Google...",
        "Finish signing in in the Google window" => "أكمل تسجيل الدخول في نافذة Google",
        "Google ended your session. Press g to sign in again." => "انتهت جلستك في Google. اضغط g لتسجيل الدخول مجدداً.",
        "Google ended your session. Sign in again." => "انتهت جلستك في Google. سجّل الدخول مجدداً.",
        "Your saved sign-in ended: continuing as a guest" => "انتهى تسجيل دخولك المحفوظ: المتابعة كضيف",
        "Still connecting to YouTube Music" => "ما زال الاتصال بيوتيوب ميوزك جارياً",
        "Sign in (Settings) to see this" => "سجّل الدخول (الإعدادات) لرؤية هذا",
        "Sign in (Settings) to see your playlists" => "سجّل الدخول (الإعدادات) لرؤية قوائمك",
        "Sign in to like songs" => "سجّل الدخول للإعجاب بالأغاني",
        "Sign in to add songs to playlists" => "سجّل الدخول لإضافة أغانٍ إلى القوائم",
        "Sign in to make playlists" => "سجّل الدخول لإنشاء قوائم",
        "Looking for lyrics..." => "جارٍ البحث عن الكلمات...",
        "Looking for lyrics…" => "جارٍ البحث عن الكلمات…",
        "Loading lyrics..." => "جارٍ تحميل الكلمات...",
        "No lyrics found" => "لم يتم العثور على كلمات",
        "No lyrics for this song" => "لا توجد كلمات لهذه الأغنية",
        "These lyrics aren't synced to the song" => "هذه الكلمات غير متزامنة مع الأغنية",
        "Lyrics from" => "الكلمات من",
        "Finding more songs..." => "جارٍ إيجاد المزيد من الأغاني...",
        "Radio has nothing new to play" => "لا يوجد شيء جديد في الراديو",
        "Queue finished, continuing with radio..." => "انتهت القائمة، المتابعة بالراديو...",
        "Plays next" => "ستُشغَّل بعد هذه",
        "Added to queue" => "أُضيفت إلى قائمة الانتظار",
        "Added to the queue" => "أُضيفت إلى قائمة الانتظار",
        "Link copied" => "تم نسخ الرابط",
        "Playing in VLC" => "يُشغَّل في VLC",
        "VLC isn't installed" => "VLC غير مثبت",
        "VLC isn't installed: playing here" => "VLC غير مثبت: التشغيل هنا",
        "Can't play" => "لا يمكن التشغيل",
        "Skipped, download failed" => "تم التخطي، فشل التنزيل",
        "Stopped after 3 songs failed" => "توقف بعد فشل 3 أغانٍ",
        "Download failed" => "فشل التنزيل",
        "Downloading" => "جارٍ تنزيل",
        "Saved to" => "تم الحفظ في",
        "Like failed" => "فشل الإعجاب",
        "Sign-in failed" => "فشل تسجيل الدخول",
        "Created" => "تم إنشاء",
        "Added to" => "أُضيفت إلى",
        "Couldn't create the playlist" => "تعذّر إنشاء القائمة",
        "No audio device" => "لا يوجد جهاز صوت",
        "No audio device found" => "لم يتم العثور على جهاز صوت",
        "Looking for songs on this device..." => "جارٍ البحث عن أغانٍ على هذا الجهاز...",
        "No songs found in your Music folder" => "لم يتم العثور على أغانٍ في مجلد الموسيقى",
        "This song is already on your device" => "هذه الأغنية موجودة على جهازك",
        "Radio is for YouTube Music songs" => "الراديو لأغاني يوتيوب ميوزك فقط",
        "Likes are for YouTube Music songs" => "الإعجاب لأغاني يوتيوب ميوزك فقط",
        "Results for" => "نتائج",
        "downloading..." => "جارٍ التنزيل...",
        "Signed in: likes and playlists sync with your phone" => "تم تسجيل الدخول: الإعجابات والقوائم متزامنة مع هاتفك",
        "Sign in for your likes, playlists and history" => "سجّل الدخول لإعجاباتك وقوائمك وسجلك",
        // ---- settings
        "Language" => "اللغة",
        "The words in the apps" => "لغة كلمات البرامج",
        "Restart the app to see everything in the new language" => "أعد تشغيل البرنامج لرؤية كل شيء باللغة الجديدة",
        "English" => "English",
        "Arabic" => "العربية",
        "Theme" => "السمة",
        "Shared with RyM Legacy Player" => "مشتركة مع RyM Legacy Player",
        "Library panel" => "لوحة المكتبة",
        "Library width" => "عرض المكتبة",
        "Lyrics panel" => "لوحة الكلمات",
        "Lyrics size" => "حجم الكلمات",
        "Lyrics text" => "محاذاة الكلمات",
        "Player bar" => "شريط التشغيل",
        "Key hints" => "تلميحات المفاتيح",
        "Audio source" => "مصدر الصوت",
        "Download quality" => "جودة التنزيل",
        "Download connections" => "اتصالات التنزيل",
        "Download piece size" => "حجم القطعة",
        "Play songs in" => "شغّل الأغاني في",
        "Reset layout" => "إعادة ضبط الترتيب",
        "Cover colors" => "ألوان الغلاف",
        "Drawing" => "الرسم",
        "On" => "تشغيل",
        "Off" => "إيقاف",
        "Shown" => "ظاهر",
        "Hidden" => "مخفي",
        "Left" => "يسار",
        "Right" => "يمين",
        "Top" => "أعلى",
        "Bottom" => "أسفل",
        "Centered" => "في المنتصف",
        "Best" => "الأفضل",
        "Fast" => "سريع",
        "Auto" => "تلقائي",
        "Best (yt-dlp, slower)" => "الأفضل (yt-dlp، أبطأ)",
        "Fast (~160 kbps)" => "سريع (~160 kbps)",
        "Best (yt-dlp)" => "الأفضل (yt-dlp)",
        "Fast (playback file)" => "سريع (ملف التشغيل)",
        "Open (split evenly)" => "مفتوح (تقسيم متساوٍ)",
        "VLC (when installed)" => "VLC (إن كان مثبتاً)",
        "RyM (built-in)" => "RyM (المدمج)",
        "Processor" => "المعالج",
        "Wash the background with a soft shade of the song's cover" => "تلوين الخلفية بلون خفيف من غلاف الأغنية",
        "Auto: YouTube's player API, yt-dlp when a song is refused" => "تلقائي: واجهة مشغل يوتيوب، وyt-dlp عند رفض أغنية",
        "Songs saved with right-click > Download go to Music/RymPlayer" => "الأغاني المحفوظة بالنقر الأيمن > تنزيل تذهب إلى Music/RymPlayer",
        "Pieces of a song downloaded at once" => "عدد أجزاء الأغنية التي تُنزَّل معاً",
        "VLC: each song opens in VLC" => "VLC: كل أغنية تُفتح في VLC",
        "VLC: each song opens in VLC. RyM: the built-in player." => "VLC: كل أغنية تُفتح في VLC. RyM: المشغل المدمج.",
        "VLC isn't installed on this PC, so songs play here." => "VLC غير مثبت على هذا الحاسوب، لذا تُشغَّل الأغاني هنا.",
        "Open: the song is split evenly over the connections, no fixed size." => "مفتوح: تُقسَّم الأغنية بالتساوي على الاتصالات، دون حجم ثابت.",
        "How many connections a song downloads over at once. YouTube limits each one, so more use more of your line." => "عدد الاتصالات التي تُنزَّل عبرها الأغنية معاً. يوتيوب يحدّ كل اتصال، فالمزيد يستخدم سرعة خطك أكثر.",
        "Best: yt-dlp with your account, 256 kbps for Premium, a few seconds more. Fast: the playback file, about 160 kbps." => "الأفضل: yt-dlp بحسابك، 256 kbps لمشتركي Premium، أبطأ بثوانٍ. سريع: ملف التشغيل، حوالي 160 kbps.",
        "InnerTube + yt-dlp: fast, and yt-dlp takes over when YouTube refuses. yt-dlp: slower start, Premium gets 256 kbps (needs yt-dlp and Node.js). InnerTube: straight from YouTube only." => "InnerTube + yt-dlp: سريع، وyt-dlp يتولى عندما يرفض يوتيوب. yt-dlp: بداية أبطأ، وPremium بجودة 256 kbps (يحتاج yt-dlp وNode.js). InnerTube: من يوتيوب مباشرة فقط.",
        "Vulkan: works on most PCs. DirectX 12: same speed, less RAM. Processor: least RAM, less smooth. Applies after a restart." => "Vulkan: يعمل على معظم الحواسيب. DirectX 12: السرعة نفسها وذاكرة أقل. المعالج: أقل ذاكرة وأقل سلاسة. يُطبَّق بعد إعادة التشغيل.",
        "↑↓ choose  ←→ change  click to change  Esc close" => "↑↓ اختيار  ←→ تغيير  انقر للتغيير  Esc إغلاق",
        "Enter add  Esc close" => "Enter إضافة  Esc إغلاق",
        "New playlist name (Enter / Esc)" => "اسم القائمة الجديدة (Enter / Esc)",
        "paste with Ctrl+V, then Enter (Esc cancels)" => "الصق بـ Ctrl+V ثم Enter (Esc للإلغاء)",
        _ => return None,
    })
}

/// True when text reads right to left (its first strong letter is Arabic, Hebrew...).
pub fn is_rtl(s: &str) -> bool {
    unicode_bidi::get_base_direction(s) == unicode_bidi::Direction::Rtl
}

/// Text as a terminal without right-to-left support should print it: Arabic letters joined
/// (their connected forms) and Arabic runs put in visual order. Other text comes back as is.
pub fn for_terminal(s: &str) -> String {
    let arabic = |c: char| matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF);
    if !s.chars().any(arabic) {
        return s.to_string();
    }
    let joined = ar_reshaper::reshape_line(s);
    let info = unicode_bidi::BidiInfo::new(&joined, None);
    let mut out = String::with_capacity(joined.len());
    for para in &info.paragraphs {
        out.push_str(&info.reorder_line(para, para.range.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_only_when_on_and_english_for_unknown_text() {
        set_arabic(false);
        assert_eq!(tr("Home"), "Home");
        set_arabic(true);
        assert_eq!(tr("Home"), "الرئيسية");
        assert_eq!(tr("Something new"), "Something new");
        set_arabic(false);
    }

    #[test]
    fn terminal_text_is_joined_and_reversed() {
        assert_eq!(for_terminal("Hello"), "Hello");
        // "سلام": joined letter forms, last letter first.
        let shown = for_terminal("سلام");
        assert_eq!(shown.chars().count(), 3, "{shown}"); // لا becomes one ligature
        assert!(shown.chars().all(|c| ('\u{FE70}'..='\u{FEFF}').contains(&c)), "{shown}");
        assert_eq!(shown.chars().last(), Some('\u{FEB3}')); // initial seen, printed last
    }
}

#[cfg(test)]
mod terminal_sample {
    /// Writes Arabic as the terminal app prints it to target/arabic-terminal.txt, one line per
    /// sample, for drawing it in different fonts: `cargo test --lib arabic_sample -- --ignored`.
    #[test]
    #[ignore]
    fn arabic_sample() {
        let lines = ["الأغاني المعجبة على هذا الجهاز", "جارٍ تنزيل الأغاني 12 / 50", "سلام عليكم يا حبيبي"];
        let out: Vec<String> = lines.iter().map(|l| super::for_terminal(l)).collect();
        std::fs::write("target/arabic-terminal.txt", out.join("\n")).unwrap();
    }
}
