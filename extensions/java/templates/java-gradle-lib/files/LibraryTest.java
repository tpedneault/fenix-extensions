package {{name_snake}};

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class LibraryTest {
    @Test
    void adds() {
        assertEquals(5, Library.add(2, 3));
    }
}
