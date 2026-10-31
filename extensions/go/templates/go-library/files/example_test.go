package {{name_snake}}_test

import (
	"fmt"

	"{{module}}/{{name_kebab}}"
)

func ExampleReverse() {
	fmt.Println({{name_snake}}.Reverse("stressed"))
	// Output: desserts
}
