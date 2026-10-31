package orbit.core;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class OrbitsTest {
    @Test
    void threeOfThem() {
        assertEquals(3, Orbits.names().size());
    }
}
