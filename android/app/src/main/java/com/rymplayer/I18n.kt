package com.rymplayer

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * The app's words in English or Arabic (Library > Language). Text is written in English in the
 * code; [t] gives the Arabic when Arabic is on. Changing it redraws the app at once.
 */
object I18n {
    var arabic by mutableStateOf(false)

    fun t(en: String): String = if (arabic) AR[en] ?: en else en

    private val AR = mapOf(
        // pages
        "Home" to "الرئيسية",
        "Search" to "بحث",
        "Library" to "المكتبة",
        "For you" to "لك",
        "History" to "السجل",
        "Downloads" to "التنزيلات",
        "Liked Music" to "الأغاني المعجبة",
        "Up next" to "التالي",
        "Now playing" to "يُشغَّل الآن",
        "Lyrics" to "الكلمات",
        "New for you" to "جديد لك",
        "Playlist" to "قائمة تشغيل",
        "Auto playlist" to "قائمة تلقائية",
        "What you played" to "ما استمعت إليه",
        "Saved on this phone, play without internet" to "محفوظة على هذا الهاتف، تعمل بدون إنترنت",
        "Songs, artists, albums" to "أغانٍ، فنانون، ألبومات",
        // actions
        "Play" to "تشغيل",
        "Shuffle" to "عشوائي",
        "Next" to "التالي",
        "Previous" to "السابق",
        "Like" to "إعجاب",
        "More" to "المزيد",
        "Start radio" to "تشغيل راديو",
        "Play next" to "تشغيل بعد هذه",
        "Add to queue" to "إضافة إلى قائمة الانتظار",
        "Remove from queue" to "إزالة من قائمة الانتظار",
        "Download" to "تنزيل",
        "Open in VLC" to "فتح في VLC",
        "Add to liked songs" to "إضافة إلى المعجبة",
        "Remove from liked songs" to "إزالة من المعجبة",
        "Save to playlist" to "حفظ في قائمة",
        "Go to album" to "الذهاب إلى الألبوم",
        "Go to artist" to "الذهاب إلى الفنان",
        "New playlist" to "قائمة جديدة",
        "Close" to "إغلاق",
        "Title" to "العنوان",
        "Create" to "إنشاء",
        "Cancel" to "إلغاء",
        "Sign in with Google" to "تسجيل الدخول بحساب Google",
        "Continue without an account" to "المتابعة بدون حساب",
        "Sign out" to "تسجيل الخروج",
        "Use another account or none" to "استخدم حساباً آخر أو بدون حساب",
        "Your playlists, likes and history" to "قوائمك وإعجاباتك وسجلك",
        "Sign in to see your playlists" to "سجّل الدخول لرؤية قوائمك",
        "YouTube Music, your way" to "يوتيوب ميوزك على طريقتك",
        // states
        "Nothing here" to "لا يوجد شيء هنا",
        "Nothing here yet. Tap to retry" to "لا يوجد شيء بعد. اضغط لإعادة المحاولة",
        "Looking for lyrics..." to "جارٍ البحث عن الكلمات...",
        "No lyrics for this song" to "لا توجد كلمات لهذه الأغنية",
        "These lyrics aren't synced to the song" to "هذه الكلمات غير متزامنة مع الأغنية",
        "Lyrics from" to "الكلمات من",
        "Your session ended. Sign in again." to "انتهت جلستك. سجّل الدخول مجدداً.",
        "Results for" to "نتائج",
        "Downloading" to "جارٍ تنزيل",
        "Download failed" to "فشل التنزيل",
        "Couldn't save the like" to "تعذّر حفظ الإعجاب",
        "Added to" to "أُضيفت إلى",
        "Couldn't add" to "تعذّرت الإضافة",
        "Created" to "تم إنشاء",
        "Couldn't create the playlist" to "تعذّر إنشاء القائمة",
        "VLC couldn't open it" to "تعذّر على VLC فتحها",
        "Can't play" to "لا يمكن التشغيل",
        "Can't start the player" to "تعذّر تشغيل المشغل",
        "Saved to Music/RymPlayer" to "تم الحفظ في Music/RymPlayer",
        // library settings
        "Language" to "اللغة",
        "The words in the app · tap to change" to "لغة كلمات التطبيق · اضغط للتغيير",
        "Download quality" to "جودة التنزيل",
        "Best" to "الأفضل",
        "Fast" to "سريع",
        "yt-dlp with your account, 256 kbps for Premium, slower" to "yt-dlp بحسابك، 256 kbps لمشتركي Premium، أبطأ",
        "About 160 kbps, in seconds · tap to change" to "حوالي 160 kbps، خلال ثوانٍ · اضغط للتغيير",
        "Play songs in" to "شغّل الأغاني في",
        "Each song opens in VLC · tap to play here" to "كل أغنية تُفتح في VLC · اضغط للتشغيل هنا",
        "The built-in player · tap to use VLC" to "المشغل المدمج · اضغط لاستخدام VLC",
        "Download connections" to "اتصالات التنزيل",
        "How many at once (1-16); more use more of your line · tap to change" to "عددها معاً (1-16)؛ الأكثر يستخدم سرعة خطك أكثر · اضغط للتغيير",
        "Download piece size" to "حجم القطعة",
        "Open" to "مفتوح",
        "No fixed size: the song is split evenly over the connections" to "بدون حجم ثابت: تُقسَّم الأغنية بالتساوي على الاتصالات",
        "Each connection fetches" to "كل اتصال يجلب",
        "at a time · tap to change" to "في كل مرة · اضغط للتغيير",
        "Theme" to "السمة",
    )
}

/** Shorthand for [I18n.t]. */
fun tr(en: String): String = I18n.t(en)
