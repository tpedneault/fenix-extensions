-- {{name}}: love . runs it.

local player = { x = 400, y = 300, speed = 200 }

function love.load()
	love.graphics.setBackgroundColor(0.1, 0.1, 0.12)
end

function love.update(dt)
	local dx, dy = 0, 0
	if love.keyboard.isDown("left", "a") then
		dx = dx - 1
	end
	if love.keyboard.isDown("right", "d") then
		dx = dx + 1
	end
	if love.keyboard.isDown("up", "w") then
		dy = dy - 1
	end
	if love.keyboard.isDown("down", "s") then
		dy = dy + 1
	end
	player.x = player.x + dx * player.speed * dt
	player.y = player.y + dy * player.speed * dt
end

function love.draw()
	love.graphics.circle("fill", player.x, player.y, 16)
	love.graphics.print("{{name}} -- arrows or WASD to move, Escape to quit", 10, 10)
end

function love.keypressed(key)
	if key == "escape" then
		love.event.quit()
	end
end
