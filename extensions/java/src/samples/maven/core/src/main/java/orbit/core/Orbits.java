package orbit.core;

import com.google.common.collect.ImmutableList;
import java.util.List;

public final class Orbits {
    private Orbits() {}

    public static List<String> names() {
        return ImmutableList.of("LEO", "MEO", "GEO");
    }
}
