ThisBuild / scalaVersion := "{{scala}}"
ThisBuild / version      := "0.1.0-SNAPSHOT"
ThisBuild / organization := "dev.{{name_snake}}"

lazy val root = (project in file("."))
  .settings(
    name := "{{name_kebab}}",
    libraryDependencies += "org.scalameta" %% "munit" % "1.2.0" % Test
  )
