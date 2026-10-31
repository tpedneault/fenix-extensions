package orbit.core

import kotlin.test.Test
import kotlin.test.assertEquals

class OrbitsTest {
    @Test
    fun threeOfThem() {
        assertEquals(3, Orbits.names().size)
    }
}
