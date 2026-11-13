package orbit_test

import "testing"

func mustBe(t *testing.T, got, want int) {
	if got != want {
		t.Fatalf("got %d, want %d", got, want)
	}
}
