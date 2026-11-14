# Samples

What the tests read:

- `orbit3.scala`, `orbit2.scala`: a Scala 3 sample (indentation syntax,
  `given`/`using`, an `enum`, extension methods, an `opaque` type, `derives`,
  `end` markers, scala-cli directives) and a Scala 2 sample (braces, implicits,
  a sealed trait, `for`/`yield`, `try`) the grammar's queries run on.
- `build.sbt`, `build.mill`: an sbt build with four projects and a Mill build
  with two modules, for the project reader.
- `javac-version.txt`, `cs-version.txt`, `metals-version.txt`,
  `scala-cli-version.txt`, `sbt-version.txt`, `mill-version.txt`: what
  `javac -version`, `cs version`, `metals --version`, `scala-cli version`,
  `sbt --version` and `mill --version` printed, recorded on Linux x86_64 from
  OpenJDK 21.0.12.1 (`javac`'s file starts with the JVM's `Picked up
  JAVA_TOOL_OPTIONS` line, as a JVM prints it, its value replaced), Coursier
  2.1.26, Metals 1.6.9, scala-cli 1.19.0, sbt's runner 2.0.12 in a project
  pinned to sbt 1.12.15, and Mill 0.11.13 (the version Coursier's `mill` app
  resolved). Coursier, Metals, scala-cli, sbt and Mill ran as their JVM
  launchers: this machine's TLS interception breaks the native Coursier and
  scala-cli's own downloads, not what they print.
- `metals-stdout.bin`: what Metals 1.6.9 wrote on its stdout, framed as LSP
  frames it, in a session with `initialize`'s options and answers the pack
  gives: in a scala-cli project (scala-cli 1.19.0's build server), `initialize`'s
  answer, `workspace/configuration`, the questions it asks with
  `showMessageRequest` (its HTTP server, a misconfigured build target, no
  `.scalafmt.conf`), progress, diagnostics, a formatting request's answer and
  `debug-adapter-start`'s. The project's path was rewritten to `/home/u/orbit`
  and the frames' lengths with it. It's a real run's, not written by hand.

The versions' output and the LSP frames are the real tools', not written by
hand.
