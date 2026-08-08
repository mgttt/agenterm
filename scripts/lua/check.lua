-- check.lua — Production task: lint directory of Lua scripts with real syntax checking.
-- Uses std.fs.read_dir, std.fs.read, pcall(load) for syntax validation.
-- Returns 0 on success, 1 on failure.
-- Usage: check.lua <dir-path>

local function check_dir(path)
    -- std.env.names for diagnostic info
    local _ = std.env.names()

    if not std.fs.exists(path) then
        print("check: directory not found: " .. path)
        return 1
    end

    local entries = std.fs.read_dir(path)
    local total = 0
    local failed = 0

    for _, entry in ipairs(entries) do
        if entry.is_file then
            local name = entry.name
            local parts = string_split(name, ".")
            local ext = parts[#parts] or ""
            if ext == "lua" then
                total = total + 1
                local ok, err = pcall(function()
                    local src = std.fs.read_to_string(entry.path)
                    -- Real syntax check via Lua's load()
                    local fn, load_err = load(src)
                    if fn == nil then
                        error(load_err)
                    end
                    return true
                end)
                if not ok or err ~= true then
                    failed = failed + 1
                    local msg = "syntax error"
                    if type(err) == "string" then msg = err end
                    print("FAIL: " .. entry.path .. " — " .. msg)
                else
                    print("OK: " .. entry.path)
                end
            end
        end
    end

    print(string.format("check: %d files checked, %d failed", total, failed))
    if failed > 0 then
        return 1
    end
    return 0
end

-- Entry point
local n = __host.args_len()
if n == 1 then
    return check_dir(__host.arg(0))
else
    return check_dir("scripts/lua")
end
