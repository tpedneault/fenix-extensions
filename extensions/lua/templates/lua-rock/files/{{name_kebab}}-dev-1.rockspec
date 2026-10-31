rockspec_format = "3.0"
package = "{{name_kebab}}"
version = "dev-1"
source = {
	url = "git+https://github.com/you/{{name_kebab}}.git",
}
description = {
	summary = "{{name}}, a Lua module",
	license = "MIT",
}
dependencies = {
	"lua >= 5.1",
}
test_dependencies = {
	"busted",
}
test = {
	type = "busted",
}
build = {
	type = "builtin",
	modules = {
		["{{name_snake}}"] = "src/{{name_snake}}.lua",
	},
}
