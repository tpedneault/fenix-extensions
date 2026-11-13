package geo

import "testing"

func TestDist(t *testing.T) {
	if Dist(1, "3") != Dist(3, 1) {
		t.Fatal("asymmetric")
	}
}

func TestPanics(t *testing.T) {
	var m map[string]int
	m["x"] = 1
}
