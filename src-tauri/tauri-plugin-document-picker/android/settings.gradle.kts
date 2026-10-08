// Standalone native compilation; the generated app supplies its own settings.
pluginManagement {
    repositories { google(); mavenCentral(); gradlePluginPortal() }
    plugins {
        id("com.android.library") version "8.11.0"
        id("org.jetbrains.kotlin.android") version "2.0.21"
    }
}
dependencyResolutionManagement {
    repositories { google(); mavenCentral() }
}
rootProject.name = "cmcloud-document-picker"
include(":tauri-android")
project(":tauri-android").projectDir = file(
    providers.gradleProperty("tauriAndroidDir").getOrElse(".tauri/tauri-api")
)
