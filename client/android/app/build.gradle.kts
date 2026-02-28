plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "codes.anb.virtue"
    compileSdk = 35

    defaultConfig {
        applicationId = "codes.anb.virtue"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

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
