@main def run(): Unit =
  println(greeting("world"))

def greeting(who: String): String =
  s"Hello, $who"
