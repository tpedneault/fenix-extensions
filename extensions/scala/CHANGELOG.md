# Changelog

## 1.0.0

- The first release, for Fenix 1.5 (extension API 1.5): Scala 2 and 3. Their
  grammar (`.scala`, `.sc`, `.sbt` and `.mill` files: highlighted, folded,
  indented, with their definitions as the scopes `SPC c s` and the breadcrumbs
  list), Metals as the language server -- found where Coursier puts it, or
  installed from the project doctor through Coursier 2.1.26, whose download is
  checked against its SHA-256 -- run by the pack so that it answers what Metals
  asks of its client (importing the build, formatting with no
  `.scalafmt.conf`) and starts Metals' debug adapter: *Scala: debug the file
  you're in* (`SPC m b`) and *debug the tests*, with the launch *Scala: Metals'
  debug session*. Formatting is Metals' (the format command), with the scalafmt
  `.scalafmt.conf` pins; on save, where the project pins scalafmt 3.11.5, a
  native scalafmt installed from the doctor. Tasks for sbt (with a run and a test
  for each project), scala-cli, Mill, Gradle and Maven; scalafix as a task where
  the build has it; a REPL in a terminal, a build that compiles as sources
  change, and scala-cli's run and test of the file you're in. The project doctor
  checks the JDK (and warns below Java 17, Metals' minimum), Coursier, Metals,
  the build tools and scalafmt. Snippets, a *Scala · sbt project* and a *Scala ·
  scala-cli script* for the new-project wizard, and a lesson in the tutorial.
  Fenix 1.6 and later show a project with a `build.sbt`, `build.sc`,
  `build.mill`, `project.scala` or `.scalafmt.conf` at its top as a Scala project
  (`SC`) in the project hub, on Home, in the modeline and in the doctor; earlier
  Fenix ignores that and the pack works there as before.
