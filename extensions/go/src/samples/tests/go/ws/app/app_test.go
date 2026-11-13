package app

import "testing"

func TestFour(t *testing.T) {
	if Four() != 4 {
		t.Fatal("no")
	}
}

func TestDouble(t *testing.T) {
	t.Run("same name as lib's", func(t *testing.T) {})
}
