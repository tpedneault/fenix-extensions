package orbit_test

import (
	"fmt"
	"testing"
	"time"

	"example.com/orbit"
)

func TestExternal(t *testing.T) {
	if orbit.Add(2, 3) != 5 {
		t.Fatal("no")
	}
}

func TestParallel(t *testing.T) {
	for _, name := range []string{"one", "two", "three"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			t.Log("working on " + name)
			time.Sleep(5 * time.Millisecond)
			if name == "two" {
				t.Errorf("%s is broken", name)
			}
		})
	}
}

func TestFromHelper(t *testing.T) {
	mustBe(t, orbit.Add(1, 1), 3)
}

func TestPrints(t *testing.T) {
	fmt.Println("hello from stdout")
	t.Log("and a log line")
	t.Log("multi\nline")
}

func TestSpecial(t *testing.T) {
	for _, name := range []string{"a+b (c)", "x|y", "[z]", "naïve"} {
		t.Run(name, func(t *testing.T) {})
	}
}

func ExampleAdd_wrong() {
	fmt.Println(orbit.Add(2, 2))
	// Output: 5
}

func ExampleAdd_silent() {
	fmt.Println("never run: no output comment")
}

func FuzzAdd(f *testing.F) {
	f.Add(1, 2)
	f.Add(-1, 1)
	f.Fuzz(func(t *testing.T, a, b int) {
		if orbit.Add(a, b) != orbit.Add(b, a) {
			t.Fatal("not commutative")
		}
	})
}
