-- mini_game.lua — the reference Lua game for GIOCHI-ENGINE.
--
-- A complete space shooter in about 200 lines: movement, shooting, drones that
-- spawn faster as the score rises, collisions, lives, game over, and a sound effect
-- synthesised at start-up so the repository carries no binary audio asset.
--
-- Run it:
--     cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua
--
-- Play it:
--     A / D or the arrow keys   move
--     Space                     shoot
--     Escape                    quit
--
-- Test it (no keyboard needed):
--     cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua --auto --frames 600
--
-- `--auto` turns on a tiny autopilot so the game can be exercised headless; the
-- score it reaches proves that movement, shooting, collisions and scoring all work.
--
-- Three flags exist for automated tests only:
--     --lives N     start with N lives (default 3)
--     --ram         steer *into* the drones, so the collision and game-over paths run
--                   without waiting for a lucky random spawn
--     --spawn SECS  use a fixed spawn interval instead of the score-driven one
--     --speed PX    use a fixed drone speed instead of the score-driven one

local giochi = require("giochi")

-- ---------------------------------------------------------------- command line

local options = {
    auto = false,
    frames = nil,
    asset_root = nil,
    lives = nil,
    ram = false,
    spawn = nil,
    speed = nil,
}

-- `arg` is set by the host: arg[0] is the script path, arg[1..] the arguments.
arg = arg or {}
for index = 1, #arg do
    local value = arg[index]
    if value == "--auto" then
        options.auto = true
    elseif value == "--frames" then
        index = index + 1
        options.frames = tonumber(arg[index])
    elseif value:match("^%-%-frames=") then
        options.frames = tonumber(value:match("=(%d+)$"))
    elseif value:match("^%-%-assets=") then
        options.asset_root = value:match("=(.+)$")
    elseif value == "--lives" then
        index = index + 1
        options.lives = tonumber(arg[index])
    elseif value == "--ram" then
        options.ram = true
    elseif value == "--spawn" then
        index = index + 1
        options.spawn = tonumber(arg[index])
    elseif value == "--speed" then
        index = index + 1
        options.speed = tonumber(arg[index])
    end
end

-- The script may live anywhere; resolve assets next to the repository by default.
local assets = options.asset_root
if not assets then
    assets = (arg[0] or ""):match("^(.*)/giochi%-engine%-lua/") or "."
    assets = assets .. "/assets/"
end

-- ------------------------------------------------------------------- constants

local WIDTH, HEIGHT = 960, 540
local SHIP_W, SHIP_H = 32, 32
local DRONE_SIZE = 28
local BULLET_W, BULLET_H = 4, 14
local SHIP_SPEED = 380
local BULLET_SPEED = 620
local DRONE_SPEED_MIN, DRONE_SPEED_MAX = 70, 150
local SPAWN_BASE, SPAWN_MIN = 1.1, 0.35
local START_LIVES = 3

local COLORS = {
    background = { 12, 14, 22 },
    ship = { 90, 200, 255 },
    bullet = { 255, 230, 120 },
    drone = { 230, 80, 80 },
    text = { 220, 220, 230 },
    bar = { 40, 160, 90 },
}

-- ------------------------------------------------------------------- the game

local function new_game()
    return {
        ship = { x = WIDTH / 2 - SHIP_W / 2, y = HEIGHT - 70 },
        bullets = {},
        drones = {},
        score = 0,
        lives = options.lives or START_LIVES,
        spawn_timer = 0,
        shoot_cooldown = 0,
        over = false,
        -- Autopilot state.
        target = nil,
    }
end

local function spawn_drone(game)
    local difficulty = math.min(game.score / 20, 1)
    game.drones[#game.drones + 1] = {
        x = math.random(0, WIDTH - DRONE_SIZE),
        y = -DRONE_SIZE,
        speed = options.speed
            or (DRONE_SPEED_MIN + (DRONE_SPEED_MAX - DRONE_SPEED_MIN) * difficulty),
    }
end

local function shoot(game)
    if game.shoot_cooldown > 0 then
        return
    end
    game.bullets[#game.bullets + 1] = {
        x = game.ship.x + SHIP_W / 2 - BULLET_W / 2,
        y = game.ship.y,
    }
    game.shoot_cooldown = 0.16
end

-- Removes an element from an array without allocating a second table.
local function remove_at(list, index)
    list[index] = list[#list]
    list[#list] = nil
end

local function overlapping(a, aw, ah, b, bw, bh)
    return a.x < b.x + bw and b.x < a.x + aw and a.y < b.y + bh and b.y < a.y + ah
end

local function autopilot(game, time)
    -- Aim at the lowest drone, and shoot whenever one is roughly above the ship.
    local target, lowest = nil, -math.huge
    for _, drone in ipairs(game.drones) do
        if drone.y > lowest then
            target, lowest = drone, drone.y
        end
    end

    local centre = game.ship.x + SHIP_W / 2
    if target then
        centre = target.x + DRONE_SIZE / 2
        -- `--ram` must not shoot: the point of that mode is to reach a collision.
        if not options.ram and math.abs(centre - (game.ship.x + SHIP_W / 2)) < 40 then
            shoot(game)
        end
    else
        centre = WIDTH / 2
    end

    -- `--ram` steers towards the drone instead of under it, which is how a test
    -- reaches the collision path without relying on a lucky spawn.
    local goal = centre - SHIP_W / 2
    if options.ram and target then
        goal = target.x + DRONE_SIZE / 2 - SHIP_W / 2
    end
    local delta = goal - game.ship.x
    local step = SHIP_SPEED * time.delta
    if math.abs(delta) < step then
        game.ship.x = goal
    else
        game.ship.x = game.ship.x + (delta > 0 and step or -step)
    end
    game.ship.x = math.max(0, math.min(WIDTH - SHIP_W, game.ship.x))
end

local function update(game, time, input, audio, beep)
    if game.over then
        -- Any key or the Escape key restarts; the autopilot restarts on its own so a
        -- headless run always ends with a reported score.
        if options.auto or input:key_pressed(giochi.keys.space) then
            return new_game()
        end
        if input:key_pressed(giochi.keys.escape) then
            giochi.quit()
        end
        return game
    end

    game.shoot_cooldown = math.max(0, game.shoot_cooldown - time.delta)

    if options.auto then
        autopilot(game, time)
    else
        if input:key_down(giochi.keys.a) or input:key_down(giochi.keys.left) then
            game.ship.x = game.ship.x - SHIP_SPEED * time.delta
        end
        if input:key_down(giochi.keys.d) or input:key_down(giochi.keys.right) then
            game.ship.x = game.ship.x + SHIP_SPEED * time.delta
        end
        game.ship.x = math.max(0, math.min(WIDTH - SHIP_W, game.ship.x))

        if input:key_down(giochi.keys.space) then
            shoot(game)
        end
    end

    -- Tick the world.
    for index = #game.bullets, 1, -1 do
        local bullet = game.bullets[index]
        bullet.y = bullet.y - BULLET_SPEED * time.delta
        if bullet.y + BULLET_H < 0 then
            remove_at(game.bullets, index)
        end
    end

    for index = #game.drones, 1, -1 do
        local drone = game.drones[index]
        drone.y = drone.y + drone.speed * time.delta

        if drone.y > HEIGHT then
            remove_at(game.drones, index)
        elseif overlapping(game.ship, SHIP_W, SHIP_H, drone, DRONE_SIZE, DRONE_SIZE) then
            remove_at(game.drones, index)
            game.lives = game.lives - 1
            if game.lives <= 0 then
                game.over = true
            end
        end
    end

    -- Bullet/drone collisions, resolved after the movement pass.
    for drone_index = #game.drones, 1, -1 do
        local drone = game.drones[drone_index]
        for bullet_index = #game.bullets, 1, -1 do
            local bullet = game.bullets[bullet_index]
            if overlapping(bullet, BULLET_W, BULLET_H, drone, DRONE_SIZE, DRONE_SIZE) then
                remove_at(game.bullets, bullet_index)
                remove_at(game.drones, drone_index)
                game.score = game.score + 1
                -- A silent engine reports an error here; the game keeps running.
                pcall(function() audio:play(beep) end)
                break
            end
        end
    end

    -- Spawn on a timer that shortens as the score grows.
    game.spawn_timer = game.spawn_timer - time.delta
    if game.spawn_timer <= 0 then
        spawn_drone(game)
        local interval = options.spawn
            or (SPAWN_BASE - math.min(game.score / 40, SPAWN_BASE - SPAWN_MIN))
        game.spawn_timer = interval
    end

    return game
end

local function draw(game, gfx, player, drone_texture)
    gfx:clear(COLORS.background)

    for _, bullet in ipairs(game.bullets) do
        gfx:rect({ bullet.x, bullet.y, BULLET_W, BULLET_H }, COLORS.bullet)
    end

    if drone_texture then
        for _, drone in ipairs(game.drones) do
            gfx:draw(drone_texture, { drone.x, drone.y, DRONE_SIZE, DRONE_SIZE })
        end
    else
        for _, drone in ipairs(game.drones) do
            gfx:rect({ drone.x, drone.y, DRONE_SIZE, DRONE_SIZE }, COLORS.drone)
        end
    end

    if player then
        gfx:draw(player, { game.ship.x, game.ship.y, SHIP_W, SHIP_H })
    else
        gfx:rect({ game.ship.x, game.ship.y, SHIP_W, SHIP_H }, COLORS.ship)
    end

    -- A health bar, drawn with rectangles: the binding has no text yet.
    for life = 1, game.lives do
        gfx:rect({ 12 + (life - 1) * 34, 12, 28, 10 }, COLORS.bar)
    end

    if game.over then
        -- A dim overlay instead of text, so a headless run still shows the state.
        gfx:rect({ 0, HEIGHT / 2 - 40, WIDTH, 80 }, { 0, 0, 0, 160 })
        gfx:rect({ WIDTH / 2 - 120, HEIGHT / 2 - 6, 240, 12 }, COLORS.text)
    end
end

-- --------------------------------------------------------------------- start-up

print(string.format("mini_game: %s, autopilot %s, assets em %s",
                    giochi.version, tostring(options.auto), assets))

local app = giochi.app {
    title = "giochi mini_game (Lua)",
    width = WIDTH,
    height = HEIGHT,
    fps = 60,
}

-- Loading happens before the loop: inside `update` the app is busy running, and the
-- renderer is only lent to the `render` callback.
--
-- PNG keeps the assets readable, but its decoder is optional at build time: the
-- console image ships an SDL_image with PNG compiled out, where `player.png` fails
-- with "unsupported image format". BMP needs no external library, so the repository
-- keeps a `.bmp` twin of every `.png` and this falls back to it. The pixels are
-- identical (scripts/png-to-bmp.py regenerates the twins).
local function primeira_textura(app, nomes)
    local ultimo
    for _, nome in ipairs(nomes) do
        local ok, valor = pcall(function() return app:texture(assets .. nome) end)
        if ok then
            return valor
        end
        ultimo = valor
    end
    error(ultimo)
end

local player = primeira_textura(app, { "player.png", "player.bmp" })
local drone_texture = primeira_textura(app, { "checker.png", "checker.bmp" })

-- A short synthesised beep, written to a temporary WAV so the repository carries no
-- binary audio asset.
local function write_beep(path)
    local rate, samples = 22050, 4410
    local file = io.open(path, "wb")
    if not file then
        return false
    end

    local function u32(value)
        return string.char(value % 256, math.floor(value / 256) % 256,
                           math.floor(value / 65536) % 256, math.floor(value / 16777216) % 256)
    end
    local function u16(value)
        return string.char(value % 256, math.floor(value / 256) % 256)
    end

    local data = {}
    for index = 0, samples - 1 do
        local envelope = 1 - index / samples
        local value = math.sin(2 * math.pi * 880 * index / rate) * envelope * 0.35
        local sample = math.floor(value * 32767)
        data[#data + 1] = u16(sample < 0 and sample + 65536 or sample)
    end
    local pcm = table.concat(data)

    file:write("RIFF", u32(36 + #pcm), "WAVE", "fmt ", u32(16), u16(1), u16(1),
               u32(rate), u32(rate * 2), u16(2), u16(16), "data", u32(#pcm), pcm)
    file:close()
    return true
end

local beep_path = os.tmpname()
local beep = nil
if write_beep(beep_path) then
    local ok, sound = pcall(function() return app:sound(beep_path) end)
    if ok then
        beep = sound
    else
        print("mini_game: som indisponível: " .. tostring(sound))
    end
    os.remove(beep_path)
end

if not app:audio_available() then
    print("mini_game: modo silencioso (nenhum dispositivo de áudio)")
end

-- ------------------------------------------------------------------- the loop

local game = new_game()
local reported = false

local function report()
    if reported then
        return
    end
    reported = true
    print(string.format("mini_game: %d frames, %d passos, %.1f fps", app:frames(),
                        app:steps(), app:fps()))
    print(string.format("pontuação final: %d", game.score))
    print(string.format("vidas restantes: %d", game.lives))
end

app:run {
    frames = options.frames,
    update = function(time, input, audio)
        game = update(game, time, input, audio, beep)
        if game.over and options.auto and options.frames then
            -- Headless runs stop at game over so the score is reported; interactive
            -- play restarts instead.
            giochi.quit()
        end
    end,
    render = function(gfx)
        draw(game, gfx, player, drone_texture)
    end,
}

report()

-- Explicit teardown: `Texture` also frees itself when collected, so this is about
-- determinism rather than correctness.
if player then player:destroy() end
if drone_texture then drone_texture:destroy() end
if beep then app:destroy_sound(beep) end

local live_textures = app:texture_count()
print(string.format("mini_game: texturas vivas: %d", live_textures))

-- Returned rather than printed, so a test can assert on it without parsing stdout.
-- `giochi-demo lua` ignores the value; a host that loads the script itself reads it.
return {
    score = game.score,
    lives = game.lives,
    over = game.over,
    frames = app:frames(),
    steps = app:steps(),
    textures = live_textures,
}
