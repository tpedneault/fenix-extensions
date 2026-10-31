package {{name_snake}};

import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import org.junit.jupiter.api.Test;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.boot.webmvc.test.autoconfigure.AutoConfigureMockMvc;
import org.springframework.test.web.servlet.MockMvc;

@SpringBootTest
@AutoConfigureMockMvc
class ApplicationTests {
    @Autowired
    MockMvc mvc;

    @Test
    void sayHello() throws Exception {
        mvc.perform(get("/hello?name=Fenix")).andExpect(status().isOk()).andExpect(jsonPath("$.message").value("Hello, Fenix"));
    }
}
