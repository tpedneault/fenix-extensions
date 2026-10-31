package server

import (
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestRoutes(t *testing.T) {
	srv := httptest.NewServer(New(slog.New(slog.NewTextHandler(io.Discard, nil))))
	defer srv.Close()

	tests := []struct {
		path   string
		status int
		want   string
	}{
		{path: "/healthz", status: http.StatusOK, want: "ok"},
		{path: "/hello/Ada", status: http.StatusOK, want: "Hello, Ada!"},
		{path: "/nowhere", status: http.StatusNotFound},
	}
	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			res, err := http.Get(srv.URL + tt.path)
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = res.Body.Close() }()
			if res.StatusCode != tt.status {
				t.Fatalf("GET %s: %d, want %d", tt.path, res.StatusCode, tt.status)
			}
			if tt.want == "" {
				return
			}
			var body map[string]string
			if err := json.NewDecoder(res.Body).Decode(&body); err != nil {
				t.Fatal(err)
			}
			if body["status"] != tt.want && body["message"] != tt.want {
				t.Errorf("GET %s: %v, want %q", tt.path, body, tt.want)
			}
		})
	}
}
