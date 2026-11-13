package util

import "testing"

func TestTriple(t *testing.T) {
	if Triple(2) != 7 {
		t.Fatal("wrong")
	}
}
