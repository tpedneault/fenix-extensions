# Diagrams

Mermaid diagrams in Fenix:

- **Drawn as you type.** `SPC m p` in a `.mmd` file opens the drawing beside it, and each change redraws it. An error shows on its line, while the last drawing that worked stays up.
- **On their own.** `SPC m v` opens the viewer. `+` `-` `0` zoom, `h` `j` `k` `l` pan, `Tab` walks the nodes (the one you're on is outlined), `Enter` goes to a node's line, and `/` finds a node.
- **Themes.** `fenix` follows the editor theme. Mermaid's own are there too, and so are yours (`diagrams.themes`). `SPC m t` writes one into the diagram; `T` on its page tries one without writing it.
- **Exports.** `SPC m e` writes an SVG or PNG beside the diagram, in the project.

Flowchart edges are drawn straight, with square corners; set `diagrams.edges` to `rounded` for Mermaid's rounded corners.

Fenix's reading view and the notebook draw their diagrams through its services (`diagrams.render`, `diagrams.svg`, `diagrams.export`).
