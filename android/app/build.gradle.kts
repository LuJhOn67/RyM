plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "com.rymplayer"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.rymplayer"
        minSdk = 29 // Android 10
        targetSdk = 35
        versionCode = 28
        versionName = "0.7.0"
    }
    // yt-dlp brings Python for each processor type: one APK per type keeps each small.
    // arm64-v8a is nearly every phone from the last years; x86_64 is the emulator.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86_64")
            isUniversalApk = false
        }
    }
    // youtubedl-android unpacks Python and yt-dlp from its native libraries at run time.
    packaging { jniLibs { useLegacyPackaging = true } }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    testOptions { unitTests.all { it.testLogging { events("passed", "failed"); showStandardStreams = true } } }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.compose.animation:animation")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.palette:palette-ktx:1.0.0")
    implementation("androidx.media3:media3-exoplayer:1.5.1")
    implementation("androidx.media3:media3-session:1.5.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("io.coil-kt:coil-compose:2.7.0")
    // Finds YouTube audio links and keeps up with YouTube's changes (GPL-3.0).
    implementation("com.github.teamnewpipe:NewPipeExtractor:v0.26.5")
    // Its JSON library, used directly by the quick link lookup (same version it brings).
    implementation("com.github.TeamNewPipe:nanojson:e9d656ddb49a412a5a0a5d5ef20ca7ef09549996")
    // Downloads with yt-dlp (Python, yt-dlp and QuickJS bundled).
    implementation("io.github.junkfood02.youtubedl-android:library:0.18.1")
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.3")
    testImplementation("junit:junit:4.13.2")
}
