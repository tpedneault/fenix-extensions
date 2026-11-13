# Changelog

## 1.1.0

- For Fenix 1.5 (extension API 1.5): how installing Alire or the Ada
  Language Server ended is a notification rather than a line in the
  message line, since the download can take a while and you may be
  elsewhere -- "Installed Alire 2.1.1", and what it's now used as; or
  "Couldn't install Alire 2.1.1", saying whether its download, unpacking
  it or setting it up failed and why, with *Try again*. The newest for a
  tool takes the last one's place, and a failure's is taken away when you
  try again. Starting an install is still said in the message line.
  GNAT, gprbuild and GNATprove are installed by `alr`, as a task, and are
  as they were. The README gives standard keys beside Vim's.

## 1.0.0

- The first release, for Fenix 1.0: ada and SPARK: their grammar, the Ada Language Server for Ada and GNAT project files, Alire's or gprbuild's tasks, GNATprove's findings as problems, the project's checks with fixes that install Alire, GNAT and the server, and snippets.
