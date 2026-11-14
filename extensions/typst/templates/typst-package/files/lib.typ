// The package's entrypoint (typst.toml's `entrypoint`): what a user gets
// from `#import "@preview/{{name_kebab}}:0.1.0": *`.

/// Sets `body` in a framed box with a title.
///
/// - title (content): the box's heading.
/// - body (content): what goes in it.
/// -> content
#let callout(title, body) = block(
  width: 100%,
  inset: 10pt,
  radius: 4pt,
  stroke: 0.6pt + luma(160),
  fill: luma(248),
)[
  #text(weight: "bold")[#title] \
  #body
]
