ThisBuild / scalaVersion := "3.7.3"
ThisBuild / organization := "dev.orbit"

lazy val root = (project in file("."))
  .aggregate(core, cli)
  .settings(name := "orbit", publish / skip := true)

lazy val core = project
  .in(file("core"))
  .settings(
    libraryDependencies += "org.typelevel" %% "cats-core" % "2.13.0",
    libraryDependencies += "org.scalameta" %% "munit" % "1.0.2" % Test
  )

lazy val cli = (project in file("cli"))
  .dependsOn(core)
  .settings(Compile / mainClass := Some("orbit.cli.Main"))

// not a project: a setting key's val
val orbitVersion = settingKey[String]("the version")

lazy val docs = project.in(file("docs")).enablePlugins(MdocPlugin)
