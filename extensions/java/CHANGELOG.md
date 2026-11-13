# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): how installing Eclipse JDT LS, the
  Java runtime it needs, java-debug, kotlin-language-server,
  google-java-format or ktlint ended is a notification rather than a line
  in the message line, since the download can take a while and you may be
  elsewhere -- "Installed Eclipse JDT LS 1.63.0", and what it's now used
  as; or "Couldn't install ...", saying whether its download, unpacking
  it or setting it up failed and why, with *Try again*. The newest for a
  tool takes the last one's place, and a failure's is taken away when you
  try again. Starting an install is still said in the message line. What
  it says when JDT LS isn't installed, and when a program is waiting for
  the debugger, names the keys as the scheme in use has them (`C-k p h`
  and `C-k u u` with standard keys, or what you or Fenix bound). It used
  to say `SPC p d` for the project doctor, which in Fenix deletes a
  project: the doctor is `SPC p h`. The README gives standard keys beside
  Vim's.

## 1.1.0

- `java.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: java and Kotlin: Eclipse JDT LS, debugging with java-debug, kotlin-language-server and Kotlin's highlighting, google-java-format and ktlint, Gradle's and Maven's tasks, the project's checks with their fixes, outdated and vulnerable dependencies, snippets, and Gradle, Spring Boot and Maven projects.
