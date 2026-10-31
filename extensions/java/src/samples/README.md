# Samples

What the tools said, for the tests:

- `gradle/`, `maven/`: Orbit, built with Gradle (a Java app, a Kotlin
  library) and with Maven (a Spring Boot module, a library); the
  wrapper's jar is made by the tests.
- `gradle-tasks-all.txt`: `./gradlew tasks --all` on `gradle/` (Gradle
  8.14.3).
- `gradle-dependency-updates.txt`: `dependencyUpdates` with the pack's
  init script (`src/gradle/dependencies.init.gradle`, versions plugin
  0.66.0) on `gradle/`.
- `mvn-dependency-list.txt`, `mvn-display-dependency-updates.txt`: Maven
  3.9.11 on `maven/` (`orbit-api` failing as Maven 3 does when the module
  it needs isn't installed).
- `jdtls-stdout.bin`: Eclipse JDT LS 1.63.0's stdout from `initialize`
  to `ServiceReady`, as it wrote it.
- `ktlint.json`: ktlint 1.8.0's `--reporter=json`.
- `dependency-check-report.json`: written by hand after
  dependency-check's JSON report template (`jsonReport.vsl`, report
  schema 1.1), since a real run first downloads NVD's database.
- `Orbit.kt`: Kotlin for the grammar's tests.
