plugins {
    java
    id("org.springframework.boot") version "4.1.2"
    id("io.spring.dependency-management") version "1.1.7"
}

group = "{{name_snake}}"
version = "0.1.0"

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of({{java}})
    }
}

repositories {
    mavenCentral()
}

dependencies {
    implementation("org.springframework.boot:spring-boot-starter-webmvc")
{{#if actuator}}
    implementation("org.springframework.boot:spring-boot-starter-actuator")
{{/if}}
    testImplementation("org.springframework.boot:spring-boot-starter-webmvc-test")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.withType<Test> {
    useJUnitPlatform()
}
