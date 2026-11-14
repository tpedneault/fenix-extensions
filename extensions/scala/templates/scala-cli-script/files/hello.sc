//> using scala {{scala}}
//> using dep com.lihaoyi::os-lib:0.11.8

// Run it with: scala-cli run hello.sc  (in Fenix, SPC m r)
val files = os.list(os.pwd).map(_.last).sorted

println(s"{{name}}: ${files.size} entries here")
files.foreach(println)
