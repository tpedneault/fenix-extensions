# Changelog

## 1.0.0

- The first release, for Fenix 1.5: spelling and grammar, offline. Harper
  (3.2.0) is added beside the language servers of the languages you name
  (`prose.languages`; Markdown to begin with), so its findings are
  problems, and is installed into the tools folder from its release, held to a
  SHA-256, by a fix row on the project doctor's page.
- A project word list, `.fenix/words.txt`, that Harper reads: `SPC m w`
  adds the word under the cursor, `SPC m W` opens the list.
- Settings, each settable for one project: on or off, the languages, how
  severe a finding is, the dialect of English, and where the word list is.
- Harper's own fixes aren't offered by `SPC c a` yet: Fenix applies only an
  action that is a plain edit, and Harper's carry a command. Plain text and
  commit messages aren't Fenix languages, so they aren't read until you map
  them to Markdown (README).
