package {{name_snake}};

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class AppTest {
    @Test
    void greets() {
        assertEquals("Hello from {{name}}", new App().greeting());
    }
}
