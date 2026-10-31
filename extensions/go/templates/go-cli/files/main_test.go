package main

import (
	"bytes"
	"testing"
)

func TestRun(t *testing.T) {
	tests := []struct {
		name    string
		args    []string
		want    string
		wantErr bool
	}{
		{name: "one", args: []string{"hello", "Ada"}, want: "Hello, Ada!\n"},
		{name: "two", args: []string{"hello", "Ada", "Grace"}, want: "Hello, Ada and Grace!\n"},
		{name: "shout", args: []string{"-shout", "hello", "Ada"}, want: "HELLO, ADA!\n"},
		{name: "nobody", args: []string{"hello"}, wantErr: true},
		{name: "unknown", args: []string{"bye"}, wantErr: true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var out bytes.Buffer
			err := run(tt.args, &out)
			if (err != nil) != tt.wantErr {
				t.Fatalf("run(%q) error = %v, want an error: %v", tt.args, err, tt.wantErr)
			}
			if !tt.wantErr && out.String() != tt.want {
				t.Errorf("run(%q) wrote %q, want %q", tt.args, out.String(), tt.want)
			}
		})
	}
}
