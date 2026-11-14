package orbit

import scala.collection.mutable.{ArrayBuffer, Map => MutableMap}
import scala.concurrent.{ExecutionContext, Future}

/** A body that orbits another. */
sealed trait Body {
  def mass: Double
}

case object Earth extends Body { val mass = 5.97e24 }
case class Probe(name: String, mass: Double) extends Body

object Orbits extends App {
  implicit val ec: ExecutionContext = ExecutionContext.global

  implicit class RichDouble(val d: Double) extends AnyVal {
    def km: Double = d / 1000.0
  }

  def period[A <: Body](body: A, radius: Double)(implicit g: Double): Double = {
    val k = 2 * math.Pi * math.sqrt(math.pow(radius, 3) / (g * body.mass))
    k
  }

  def describe(b: Body): String = b match {
    case Earth => "home"
    case Probe(n, m) if m > 100.0 => s"heavy probe $n"
    case Probe(n, _) => s"probe $n"
  }

  val names = for {
    b <- List(Earth, Probe("p1", 12.5))
    if b.mass > 10
  } yield describe(b)

  var total = 0
  for (i <- 1 to 3) total += i

  lazy val later: Future[Int] = Future { 21 * 2 }

  try {
    println(s"""total ${total}
               |names ${names.mkString(",")}""".stripMargin)
  } catch {
    case e: Exception => throw new IllegalStateException("bad", e)
  } finally {
    println('done)
  }
}
