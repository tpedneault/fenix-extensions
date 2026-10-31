package orbit.app;

import com.google.common.base.Joiner;
import orbit.core.Orbits;

public class App {
    public static void main(String[] args) {
        System.out.println(Joiner.on(", ").join(Orbits.INSTANCE.names()));
    }
}
