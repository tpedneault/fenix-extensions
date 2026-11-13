# Changelog

## 1.1.0

- For Fenix 1.5 (extension API 1.5): how installing Phpactor ended is a
  notification rather than a line in the message line, since the
  download can take a while and you may be elsewhere -- "Installed
  Phpactor 2026.10.07.0", and the folder it's in; or "Couldn't install
  Phpactor 2026.10.07.0", saying whether its download, unpacking it or
  setting it up failed and why, with *Try again*. A failure's is taken
  away when you try again. Starting the install is still said in the
  message line. Installing Intelephense (npm) and the project's packages
  (Composer) is run as a task, and is as it was. The README gives
  standard keys beside Vim's. On Fenix 1.0 to 1.4, 1.0.0 stays the one
  you have.

## 1.0.0

- The first release, for Fenix 1.0: pHP: tree-sitter-php's highlighting, Intelephense or Phpactor, the project's Pint or PHP CS Fixer as the formatter, PHPStan on save, Composer's scripts and the project's tools as tasks, the project's checks with their fixes, snippets, and a Composer package project.
