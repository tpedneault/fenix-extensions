package orbit

import (
	"fmt"
	"testing"
)

func TestAdds(t *testing.T) {
	if Add(1, 2) != 3 {
		t.Fatal("no")
	}
}

func TestFailsOnPurpose(t *testing.T) {
	t.Log("some output before the failure")
	if got := Add(2, 2); got != 5 {
		t.Errorf("Add(2, 2) = %d, want 5", got)
	}
}

func TestSkipped(t *testing.T) {
	t.Skip("slow")
}

func TestTable(t *testing.T) {
	cases := []struct {
		name    string
		a, b, w int
	}{
		{"small", 1, 1, 2},
		{"negative one", -1, -1, -3},
		{"a/b", 2, 2, 4},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := Add(c.a, c.b); got != c.w {
				t.Errorf("got %d, want %d", got, c.w)
			}
		})
	}
}

func TestNested(t *testing.T) {
	t.Run("outer", func(t *testing.T) {
		t.Run("inner", func(t *testing.T) {})
	})
}

func ExampleAdd() {
	fmt.Println(Add(1, 1))
	// Output: 2
}

func BenchmarkAdd(b *testing.B) {
	for i := 0; i < b.N; i++ {
		Add(1, 2)
	}
}
