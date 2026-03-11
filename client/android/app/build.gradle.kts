import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

data class VersionInfo(
    val baseVersion: String,
    val buildLabel: String,
    val androidVersionCode: Int
)

fun loadVersionInfo(clientRoot: File): VersionInfo {
    val versionFile = clientRoot.resolve("version.properties")
    val properties = Properties()
    versionFile.inputStream().use { properties.load(it) }

    val baseVersion = properties.getProperty("VERSION")
        ?: error("VERSION missing from ${versionFile.absolutePath}")
    val androidVersionCode = properties.getProperty("ANDROID_VERSION_CODE")?.toInt()
        ?: error("ANDROID_VERSION_CODE missing from ${versionFile.absolutePath}")
    val buildDate = System.getenv("VIRTUE_BUILD_DATE")
        ?: ZonedDateTime.now(ZoneOffset.UTC).format(DateTimeFormatter.ISO_LOCAL_DATE)
    val gitShortHash = System.getenv("VIRTUE_GIT_SHORT_HASH")
        ?: System.getenv("GITHUB_SHA")?.take(7)
        ?: run {
            val process = ProcessBuilder(
                "git",
                "-C",
                clientRoot.parentFile.absolutePath,
                "rev-parse",
                "--short",
                "HEAD"
            )
                .redirectErrorStream(true)
                .start()
            val output = process.inputStream.bufferedReader().use { it.readText() }.trim()
            check(process.waitFor() == 0) {
                "git rev-parse failed while loading Android version info: $output"
            }
            output
        }

    return VersionInfo(
        baseVersion = baseVersion,
        buildLabel = "$baseVersion-dev-$buildDate-$gitShortHash",
        androidVersionCode = androidVersionCode
    )
}

val versionInfo = loadVersionInfo(rootDir.parentFile)

android {
    namespace = "org.virtueinitiative.virtue"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.virtueinitiative.virtue"
        minSdk = 29
        targetSdk = 35
        versionCode = versionInfo.androidVersionCode
        versionName = versionInfo.buildLabel
        buildConfigField("String", "VIRTUE_BUILD_LABEL", "\"${versionInfo.buildLabel}\"")
        buildConfigField("String", "VIRTUE_BASE_VERSION", "\"${versionInfo.baseVersion}\"")

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        buildConfig = true
        viewBinding = true
    }

    sourceSets {
        getByName("main").jniLibs.srcDirs("src/main/jniLibs")
    }

    ndkVersion = "26.1.10909125"
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.activity:activity-ktx:1.9.2")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.6")
    implementation("androidx.work:work-runtime-ktx:2.9.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
}

val buildRustRelease by tasks.registering(Exec::class) {
    group = "rust"
    description = "Build Rust JNI library for Android"

    workingDir = rootDir
    commandLine(
        "bash", "-lc",
        """
        export ANDROID_SDK_ROOT="${'$'}{ANDROID_SDK_ROOT:-${'$'}HOME/Android/Sdk}"
        export ANDROID_HOME="${'$'}ANDROID_SDK_ROOT"
        export PATH="${'$'}HOME/.cargo/bin:${'$'}PATH"
        cargo ndk -t arm64-v8a -t x86_64 -o app/src/main/jniLibs build --release --locked --manifest-path rust/Cargo.toml
        """.trimIndent()
    )
}

tasks.named("preBuild").configure {
    dependsOn(buildRustRelease)
}
