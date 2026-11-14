// The document. SPC m c compiles it to main.pdf and shows it in the PDF
// reader; SPC m w keeps it compiled as you save.
#set document(title: "{{name}}", author: "{{author}}")
#set page(paper: "{{paper}}", margin: 2.5cm, numbering: "1")
#set text(size: 11pt, lang: "en")
#set par(justify: true)
#set heading(numbering: "1.1")
#set math.equation(numbering: "(1)")

#align(center)[
  #text(size: 1.8em, weight: "bold")[{{name}}] \
  #v(0.5em)
  {{author}} \
  #datetime.today().display("[month repr:long] [day], [year]")
]

#v(1em)
#align(center)[
  #block(width: 85%)[
    *Abstract.* Say in a few sentences what this article shows and why it
    matters.
  ]
]

= Introduction

Typst reads like text, with a few marks: *strong*, _emphasis_, a list,

- an item,
- and another,

and a reference to a figure (@fig:sine), an equation (@eq:euler) and a
source (@knuth84).

= Method

Equations sit between dollar signs; with spaces inside, they're set on a
line of their own:

$ e^(i pi) + 1 = 0 $ <eq:euler>

#figure(
  rect(width: 60%, height: 3cm, fill: luma(240))[A figure goes here.],
  caption: [Replace this box with `image("plot.png")`.],
) <fig:sine>

= Results

#figure(
  table(
    columns: 3,
    align: (left, right, right),
    table.header[*Run*][*Time (s)*][*Error*],
    [one], [1.2], [0.01],
    [two], [0.9], [0.02],
  ),
  caption: [A table with a header row.],
)

= Conclusion

What follows from the results.

#bibliography("refs.bib", style: "ieee")
