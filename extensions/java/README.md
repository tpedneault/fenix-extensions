# Java and Kotlin

Java and Kotlin in Fenix: Eclipse JDT LS as Java's language server,
debugging with java-debug, kotlin-language-server (or JetBrains'
kotlin-lsp) and Kotlin's highlighting, google-java-format and ktlint,
Gradle's and Maven's tasks, the project doctor's checks with their
fixes, outdated and vulnerable dependencies, snippets, and Gradle,
Spring Boot and Maven projects in the new-project wizard. Built on
lang-kit (`crates/lang-kit`), as each language pack is.

It reads the build: Gradle's `settings.gradle(.kts)`, each project's
`build.gradle(.kts)` and the conventions in `buildSrc`, or Maven's
`pom.xml` and its modules. Each module's application plugin and main
class, Kotlin, Spring Boot, and the Java release the build asks for (a
toolchain, `jvmToolchain`, `sourceCompatibility`,
`maven.compiler.release`) come from there. Gradle's files are code, so
they're read for the forms people write, not run.

## Java's language server

The pack is Java's language server to Fenix, and runs Eclipse JDT LS
1.63.0 behind it, passing each message on. That way it gives JDT LS
java-debug's bundle and the project's JDK (`java.configuration.runtimes`,
Gradle's Java home) in `initialize`, and asks it things of its own:
java-debug's port, the main classes, a main class's classpath.

The doctor's fix rows install JDT LS (the milestone's tarball, its
SHA-256 checked) and, since it runs on Java 25 or later, Temurin's JRE 25
when the machine's Java is older. That runtime is called `fenix-java` in
the tools folder, so that `java` is still the project's. JDT LS's
workspace is the pack's own folder's (`${data}/jdtls-workspace/<project>`),
apart from what it installs. Eclipse's `.project`, `.classpath` and
`.settings` stay in that workspace; JDT LS's own build goes to `bin/`.

## Kotlin

Fenix doesn't highlight Kotlin itself: the pack brings tree-sitter-kotlin's
grammar compiled to WebAssembly (`grammars/`, its source and licence
there), with highlighting, folds and indents (`queries/kotlin`). `*.kt` and
`*.kts` are Kotlin, Gradle's build scripts too.

The server is JetBrains' `kotlin-lsp --stdio` when it's installed
(Homebrew's, or its script on `PATH`), else kotlin-language-server 1.3.13,
which the doctor installs. kotlin-lsp isn't installed by the pack: it's
partly closed-source, its macOS archive is a `.sit`, and it's still
alpha. kotlin-language-server is no longer developed; its Kotlin 2.1 can't
run on Java 25, so it runs on the project's `java` (`java.jdk` says
which), and the doctor says so when that's 25.

## Formatting and linting

`SPC c F` -- and saving, with `editor.format_on_save` -- runs
google-java-format on a Java buffer (`google-java-format -`: its native
build where there is one, else its jar) and ktlint on a Kotlin one
(`ktlint --format --stdin --stdin-path=${file}`). Without
google-java-format, JDT LS formats. ktlint ends badly while there's a
problem it can't fix, but what it could fix is taken all the same; saving
a Kotlin file runs ktlint as a linter (`java.ktlint_on_save`), which
says what's left.
JDT LS and the Kotlin server give their own problems.

## Debugging

java-debug is a JDT LS plugin: once JDT LS has read the project, the pack
asks it to start java-debug (`vscode.java.startDebugSession`), and Fenix
connects to the port it says. java-debug's launch needs the classpath,
which a launch file can't carry, so every session attaches, on 5005:

| | |
|---|---|
| Java: the main class | *Java: debug a main class* (`SPC m m`) runs the main class you're in (or picks one of JDT LS's) held for the debugger; an application's own main class runs by Gradle's `run --debug-jvm`, which builds the Kotlin JDT LS doesn't |
| Java: the tests | *Java: debug the tests* (`SPC m t`): the test class you're in, else its module's, by `./gradlew test --debug-jvm` or `mvn test -Dmaven.surefire.debug` |
| Java: attach (remote, port 5005) | a JVM started with `-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005` |

Each runs as a task; `SPC u u` and the launch file then attach. The
adapter is Java's: breakpoints in Kotlin files weren't tried, and
java-debug finds a breakpoint's class through JDT LS, which doesn't read
Kotlin.

## Tasks

`SPC p t`: Gradle's `build`, `test`, `clean` and `check`, each project's
`run` (`bootRun` for Spring Boot) and `test`, and -- once `gradle tasks
--all` has said what there is -- the build's own (a group of its own,
`bootJar`, `installDist`, `javadoc`). Maven's: `clean`, `compile`,
`test`, `package`, `verify`, `install`, `clean install`, each module's
`test` and `package`, and `spring-boot:run` or `exec:java` for one that
runs. They run the wrapper when it's there and can run (`gradlew` with its
jar, `mvnw`; `gradlew.bat` and `mvnw.cmd` on Windows), else Gradle or
Maven. A task of the same name in
`.fenix/tools.json` wins. Saving a build file reads the build again.

## The project doctor

- **JDK**: `javac -version`, held to the release the build asks for. Fix:
  Temurin's download page for it.
- **Gradle wrapper** / **Maven wrapper**: there, and its Gradle able to
  run on the JDK (Gradle 8.14 runs on Java 24 at most, 9.1 on 25). Fix:
  `gradle wrapper --gradle-version 9.8.2`; Maven's script-only wrapper.
- **Server runtime**: Java 25 for JDT LS. Fix: Temurin's JRE 25.
- **Language server**: JDT LS, installed, starting or ready. Fix:
  install it.
- **Debugger**: java-debug 0.53.1. Fix: install it.
- **google-java-format**, and for Kotlin **Kotlin server** and **ktlint**.
  Fix: install each.

## Packages

`SPC m p` (or *Java: dependencies*): lang-kit's page.

| | Gradle | Maven |
|---|---|---|
| list, outdated | the versions plugin's report (`dependencyUpdates`) -- the build's own plugin, or one the pack's init script applies -- as JSON | `dependency:list`, `versions:display-dependency-updates` |
| vulnerable | OWASP dependency-check's JSON report (`dependencyCheckAggregate`) | `dependency-check:aggregate` |
| `u`, `U` | the init script's `fenixEdit`: the version in the build file or the version catalog | `versions:use-dep-version`, `update-property` for a property's; `use-latest-releases` |
| `a` | `fenixEdit` adds it to the module you're in's `dependencies { }`, at its newest release if none is given | `dependency:get`: Maven has no command that edits pom.xml, so it's fetched, for you to add |
| `d` | `fenixEdit` takes it out of the build file that declares it | -- |

Pre-releases aren't offered. dependency-check runs when the build has it,
or with `java.dependency_check`: its first run downloads NVD's database,
which takes a while.

## Snippets

Java: `cls`, `rec`, `iface`, `enum`, `sealed`, `main`, `mainc`, `ctor`,
`sout`, `fori`, `foreach`, `try`, `tryr`, `sw` (a switch expression),
`stream`, `groupby`, `opt`, `log`; JUnit 5's `test`, `ptest`, `tclass`,
`before`, `athrows`. Kotlin: `data`, `cls`, `sealed`, `enum`, `obj`,
`comp`, `main`, `fun`, `ext`, `when`, `lazy`, `test`; coroutines' `rb`,
`launch`, `async`, `sfun`, `scope`, `wctx`, `flow`, `ctest`.

## Project templates

- **Gradle application**, **Gradle library**: Java or Kotlin, Kotlin DSL,
  a toolchain (21, 25 or 17; the foojay resolver downloads it), JUnit 5.
- **Spring Boot**: Spring Boot 4.1 on Gradle, a JSON endpoint and its
  MockMvc test, Actuator if you like.
- **Maven**: a Java app, its release, JUnit 5, `exec:java`, the versions
  plugin.

Each makes its wrapper with the Gradle or Maven installed (`needs`), as
the wizard can't write the wrapper's jar.

## Keys

After `SPC m` in a Gradle or Maven project: `p` dependencies, `d` check
again, `m` debug a main class, `t` debug the tests, `r` read the build
again.

## Not yet

- Java's language server is the pack's, so Fenix's `lsp.servers` for Java
  doesn't replace it.
