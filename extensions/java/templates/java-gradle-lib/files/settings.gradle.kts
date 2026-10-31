plugins {
    // Downloads the JDK the toolchain asks for when it isn't installed.
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

rootProject.name = "{{name_kebab}}"
include("lib")
