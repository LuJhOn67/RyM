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
