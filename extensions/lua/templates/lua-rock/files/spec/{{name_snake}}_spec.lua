local {{name_snake}} = require("{{name_snake}}")

describe("{{name_snake}}", function()
	it("greets", function()
		assert.are.equal("Hello, Ada!", {{name_snake}}.greet("Ada"))
	end)
end)
