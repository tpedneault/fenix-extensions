// Command {{name_kebab}} greets whoever it's told to.
package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"
)

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "{{name_kebab}}:", err)
		os.Exit(2)
	}
}

// run is the program, given its arguments and where to write: main's
// only job is to call it, so the tests can too.
func run(args []string, out io.Writer) error {
	fs := flag.NewFlagSet("{{name_kebab}}", flag.ContinueOnError)
	fs.SetOutput(out)
	shout := fs.Bool("shout", false, "say it in capitals")
	fs.Usage = func() {
		_, _ = fmt.Fprintln(fs.Output(), "usage: {{name_kebab}} [-shout] hello <name>...")
		fs.PrintDefaults()
	}
	if err := fs.Parse(args); err != nil {
		return err
	}
	switch fs.Arg(0) {
	case "hello":
		names := fs.Args()[1:]
		if len(names) == 0 {
			return errors.New("hello: whom to greet?")
		}
		line := "Hello, " + strings.Join(names, " and ") + "!"
		if *shout {
			line = strings.ToUpper(line)
		}
		_, err := fmt.Fprintln(out, line)
		return err
	case "":
		fs.Usage()
		return nil
	default:
		return fmt.Errorf("%s: no such command", fs.Arg(0))
	}
}
