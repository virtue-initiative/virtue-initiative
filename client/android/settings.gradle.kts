pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

// rustls-platform-verifier ships its Kotlin half as a local Maven repository
// inside the `rustls-platform-verifier-android` crate source, so the path
// depends on which version cargo resolved. Ask cargo where that crate is
// rather than hardcoding a registry path. Declared here because
// FAIL_ON_PROJECT_REPOS forbids per-project `repositories {}` blocks.
fun rustlsPlatformVerifierRepo(): java.io.File {
    val metadata = providers.exec {
        workingDir = rootDir
        commandLine(
            "cargo", "metadata", "--format-version", "1",
            "--filter-platform", "aarch64-linux-android",
            "--manifest-path", "rust/Cargo.toml"
        )
    }.standardOutput.asText.get()

    @Suppress("UNCHECKED_CAST")
    val packages = groovy.json.JsonSlurper().parseText(metadata)
        .let { it as Map<String, Any> }["packages"] as List<Map<String, Any>>
    val manifestPath = packages
        .firstOrNull { it["name"] == "rustls-platform-verifier-android" }
        ?.get("manifest_path") as String?
        ?: error("rustls-platform-verifier-android missing from the cargo dependency graph")

    return java.io.File(java.io.File(manifestPath).parentFile, "maven")
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        maven {
            url = rustlsPlatformVerifierRepo().toURI()
            // Only this one group lives here; keep Gradle from probing a
            // cargo-registry path for every other dependency.
            content { includeGroup("rustls") }
        }
    }
}

rootProject.name = "VirtueAndroid"
include(":app")
