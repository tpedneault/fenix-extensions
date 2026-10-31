package orbit.core

import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking

/** An orbit, by its period. */
@JvmInline
value class Seconds(val value: Long)

data class Orbit(val name: String, val period: Seconds, val crew: Int = 0) {
    fun describe(): String = "$name: ${period.value}s, crew of $crew"

    companion object {
        const val MAX = 42
    }
}

enum class Status { NOMINAL, DEGRADED }

interface Tracker {
    suspend fun track(orbit: Orbit): Status
}

object Registry : Tracker {
    private val known = mutableListOf<Orbit>()

    override suspend fun track(orbit: Orbit): Status {
        known += orbit
        return when {
            orbit.crew > 3 -> Status.DEGRADED
            else -> Status.NOMINAL
        }
    }
}

fun main(args: Array<String>) = runBlocking {
    // Two at once.
    val a = async { Registry.track(Orbit("ISS", Seconds(5520L), crew = 7)) }
    val names = listOf("a", "b").map { it.uppercase() }.filter { it != "B" }
    for (i in 0 until 3) {
        if (i % 2 == 0) println("even $i") else println('x')
    }
    val x: Double? = 1.5e3
    println(a.await() to names.size + (x ?: 0.0).toInt())
}
