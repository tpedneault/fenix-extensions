package {{name_snake}}

import "testing"

func TestReverse(t *testing.T) {
	tests := []struct {
		name string
		in   string
		want string
	}{
		{name: "empty", in: "", want: ""},
		{name: "ascii", in: "hello", want: "olleh"},
		{name: "accents", in: "héllo", want: "olléh"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := Reverse(tt.in); got != tt.want {
				t.Errorf("Reverse(%q) = %q, want %q", tt.in, got, tt.want)
			}
		})
	}
}
