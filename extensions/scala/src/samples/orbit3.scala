//> using scala 3.7.3
//> using dep org.typelevel::cats-core:2.13.0
package orbit

import scala.collection.mutable.{ArrayBuffer, Map as MutableMap}
import scala.util.*

/** A body that orbits another.
  * Uses [[Vec]] for its position.
  */
opaque type Metres = Double

object Metres:
  def apply(value: Double): Metres = value
  extension (m: Metres)
    def km: Double = m / 1000.0
    def +(other: Metres): Metres = m + other

enum Body(val mass: Double) derives CanEqual:
  case Earth extends Body(5.97e24)
  case Moon extends Body(7.35e22)
  case Probe(name: String, kg: Double) extends Body(kg)

  def heavy: Boolean = mass > 1e23
end Body

trait Show[A]:
  def show(a: A): String

given Show[Int] with
  def show(a: Int): String = s"int $a"

given Show[String] = new Show[String]:
  def show(a: String) = a.toUpperCase

def render[A](a: A)(using s: Show[A]): String =
  s.show(a)

extension [A](xs: List[A])
  def second: Option[A] = xs.drop(1).headOption

case class Station(name: String, lat: Double, lon: Double)

class Fleet(stations: Seq[Station]) extends Iterable[Station]:
  private val byName = stations.map(s => s.name -> s).toMap
  def iterator: Iterator[Station] = stations.iterator

  inline def find(name: String): Option[Station] =
    byName.get(name)

  def describe(s: Station): String = s match
    case Station("kir", _, _) => "Kiruna"
    case Station(n, lat, _) if lat > 60.0 => s"$n in the north"
    case _ => "elsewhere"

@main def run(args: String*): Unit =
  val fleet = Fleet(List(Station("kir", 67.8, 20.2), Station("wal", 37.9, -75.5)))
  val names = for
    s <- fleet
    if s.lat > 0
  yield s.name
  var count = 0
  while count < 3 do
    count += 1
  if names.nonEmpty then println(names.mkString(", ")) else println("none")
  Try(render(42)) match
    case Success(text) => println(text)
    case Failure(e)    => println(s"failed: ${e.getMessage}")
  println(Body.Earth.heavy)
end run
