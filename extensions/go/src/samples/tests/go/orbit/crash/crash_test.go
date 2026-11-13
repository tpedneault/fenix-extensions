package crash

import (
	"os"
	"testing"
)

func TestBefore(t *testing.T) {
	t.Log("fine so far")
}

func TestExits(t *testing.T) {
	os.Exit(3)
}

func TestAfter(t *testing.T) {}
