module example.com/tool

go 1.21

require github.com/spf13/cobra v1.8.0

require (
	golang.org/x/sync v0.6.0
	github.com/inconshreveable/mousetrap v1.1.0 // indirect
	github.com/google/uuid v1.5.0
)

replace golang.org/x/sync => golang.org/x/sync v0.6.0
