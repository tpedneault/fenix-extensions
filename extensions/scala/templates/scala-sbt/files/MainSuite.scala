class MainSuite extends munit.FunSuite {
  test("the greeting says who it's for") {
    assertEquals(greetingFor("world"), "Hello, world")
  }

  private def greetingFor(who: String): String = s"Hello, $who"
}
