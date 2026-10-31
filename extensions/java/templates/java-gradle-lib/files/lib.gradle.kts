plugins {
{{#if language == kotlin}}
    kotlin("jvm") version "2.4.21"
{{/if}}
    `java-library`
}

group = "{{name_snake}}"
version = "0.1.0"

repositories {
    mavenCentral()
}

dependencies {
    testImplementation(platform("org.junit:junit-bom:6.1.3"))
    testImplementation("org.junit.jupiter:junit-jupiter")
{{#if language == kotlin}}
    testImplementation(kotlin("test"))
{{/if}}
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

java {
{{#if language == java}}
    toolchain {
        languageVersion = JavaLanguageVersion.of({{java}})
    }
{{/if}}
    withSourcesJar()
    withJavadocJar()
}
{{#if language == kotlin}}

kotlin {
    jvmToolchain({{java}})
}
{{/if}}

tasks.test {
    useJUnitPlatform()
}
