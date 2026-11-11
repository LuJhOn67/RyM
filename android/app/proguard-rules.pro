# The stream extractor runs YouTube's player JavaScript with Rhino, which uses reflection.
-keep class org.mozilla.javascript.** { *; }
-keep class org.schabi.newpipe.extractor.** { *; }
-dontwarn java.beans.**
-dontwarn javax.script.**
-dontwarn org.mozilla.javascript.**

# youtubedl-android reads yt-dlp's answers with Jackson (reflection).
-keep class com.yausername.** { *; }
-keep class com.fasterxml.jackson.** { *; }
-dontwarn com.fasterxml.jackson.**
# It unpacks Python and yt-dlp with Commons Compress, which makes its zip field types by reflection.
-keep class org.apache.commons.compress.** { *; }
-dontwarn org.apache.commons.compress.**

# WorkManager's database is a Room database it makes by reflection (its no-argument
# constructor); R8 from Android Gradle Plugin 9 removed that constructor, and the app closed at
# launch. Every Room database keeps its constructor.
-keep class * extends androidx.room.RoomDatabase { <init>(); }
