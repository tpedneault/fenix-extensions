package slow

import (
	"testing"
	"time"
)

func TestQuick(t *testing.T) {}

func TestSlow(t *testing.T) {
	time.Sleep(2 * time.Second)
}
