# Kotlin

Kotlin in Fenix. It lives in the Java and Kotlin pack, which shares the
JVM, Gradle and the project doctor between the two; this bundle is that
pack under Kotlin's name. One install on the Extensions page's Bundles
(`SPC h x`) brings it.

| Extension | What |
|---|---|
| `fenix.java` | Kotlin's highlighting, folds and indents (tree-sitter-kotlin); JetBrains' kotlin-lsp when it's installed, else kotlin-language-server, which the doctor installs; ktlint formatting and linting on save; Gradle's tasks; Kotlin snippets; Gradle application and library projects in the Kotlin DSL. Java too: JDT LS, java-debug, google-java-format, Maven |

`fenix.java`'s README says the rest. Removing the bundle removes it too,
unless you installed it on its own or another extension needs it.
