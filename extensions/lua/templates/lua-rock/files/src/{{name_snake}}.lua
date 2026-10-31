--- {{name}}.
-- @module {{name_snake}}

local M = {}

--- Greets `name`.
---@param name string
---@return string
function M.greet(name)
	return ("Hello, %s!"):format(name)
end

return M
