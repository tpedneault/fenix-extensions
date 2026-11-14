// A report: markup, code and math, for the grammar's queries.
#import "@preview/cetz:0.3.0": canvas
#import "lib.typ": callout
#set page(paper: "a4", margin: 2cm)
#set text(font: "Libertinus Serif", size: 11pt)
#let accent = rgb("#1a73e8")
#let double(x) = x * 2
#show heading: it => text(fill: accent)[#it]

= Introduction <intro>
== A second level
This is *strong*, _emphasised_, `raw`, and a link https://typst.app here. See @intro
and @knuth84. A line break \ and an escape \# here.

- an item
+ a numbered one
/ Term: its description

#for i in range(3) {
  [Item #i: #double(i)]
}
#if true { "yes" } else { "no" }
#figure(image("fig.png", width: 50%), caption: [A picture]) <fig:one>

```rust
fn main() {}
```

$ sum_(k=1)^n k = (n (n + 1)) / 2 $
Inline math $x^2 + y_1$ in text.
#context counter(page).display()
#callout(title: "Note")[Hello.]
