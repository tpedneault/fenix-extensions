// Command {{name_kebab}} serves the API.
package main

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"{{module}}/{{name_kebab}}/internal/server"
)

func main() {
	log := slog.New(slog.NewTextHandler(os.Stderr, nil))
	addr := os.Getenv("ADDR")
	if addr == "" {
		addr = "127.0.0.1:8080"
	}
	srv := &http.Server{
		Addr:              addr,
		Handler:           server.New(log),
		ReadHeaderTimeout: 5 * time.Second,
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		log.Info("listening", "addr", addr)
		if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Error("serving", "err", err)
			stop()
		}
	}()
	<-ctx.Done()

	// Requests under way get 10 seconds to finish.
	shutdown, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := srv.Shutdown(shutdown); err != nil {
		log.Error("shutting down", "err", err)
		os.Exit(1)
	}
}
