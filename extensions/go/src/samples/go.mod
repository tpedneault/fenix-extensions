module example.com/orbit

go 1.22

toolchain go1.24.7

require (
	golang.org/x/text v0.3.5
	gopkg.in/yaml.v2 v2.2.2
)

require github.com/google/uuid v1.3.0 // indirect

require github.com/spf13/pflag v1.0.5

replace github.com/spf13/pflag => github.com/spf13/pflag v1.0.3
