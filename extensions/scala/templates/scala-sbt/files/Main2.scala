object Main extends App {
  println(greeting("world"))

  def greeting(who: String): String =
    s"Hello, $who"
}
