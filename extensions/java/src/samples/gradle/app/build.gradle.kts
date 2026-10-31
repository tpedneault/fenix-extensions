plugins {
    application
}

repositories {
    mavenCentral()
}

dependencies {
    implementation(project(":core"))
    implementation("com.google.guava:guava:33.0.0-jre")
    testImplementation(platform(libs.junit.bom))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(21)
    }
}

application {
    mainClass = "orbit.app.App"
}

tasks.test {
    useJUnitPlatform()
}
