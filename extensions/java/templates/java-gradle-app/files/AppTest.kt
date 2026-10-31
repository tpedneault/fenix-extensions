package {{name_snake}}

import kotlin.test.Test
import kotlin.test.assertEquals

class AppTest {
    @Test
    fun greets() {
        assertEquals("Hello from {{name}}", greeting())
    }
}
