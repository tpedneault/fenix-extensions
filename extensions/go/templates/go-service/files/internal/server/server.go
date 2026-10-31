// Package server is the service's HTTP API.
package server

import (
	"encoding/json"
	"log/slog"
	"net/http"
	"time"
)

// New is the service's handler: its routes, each request logged.
func New(log *slog.Logger) http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", health)
	mux.HandleFunc("GET /hello/{name}", hello)
	return logged(log, mux)
}

func health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

// Greeting is what /hello/{name} answers.
type Greeting struct {
	Message string `json:"message"`
}

func hello(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, Greeting{Message: "Hello, " + r.PathValue("name") + "!"})
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func logged(log *slog.Logger, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		next.ServeHTTP(w, r)
		log.Info("request", "method", r.Method, "path", r.URL.Path, "took", time.Since(start))
	})
}
