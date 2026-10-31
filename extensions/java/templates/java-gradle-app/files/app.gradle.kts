plugins {
{{#if language == kotlin}}
    kotlin("jvm") version "2.4.21"
{{/if}}
    application
}

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

{{#if language == kotlin}}
kotlin {
    jvmToolchain({{java}})
}
{{else}}
java {
    toolchain {
        languageVersion = JavaLanguageVersion.of({{java}})
    }
}
{{/if}}

application {
{{#if language == kotlin}}
    mainClass = "{{name_snake}}.AppKt"
{{else}}
    mainClass = "{{name_snake}}.App"
{{/if}}
}

tasks.test {
    useJUnitPlatform()
}
