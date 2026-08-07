-- check.lua — Production task example: lint directory of Lua scripts.
-- Demonstrates std.fs.read_dir, std.fs.exists, string_split, print.
-- Returns 0 on success, non-zero on failure.
-- Usage: check.lua <dir-path>

local function check_dir(path)
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
            -- Only check .lua files
            local parts = string_split(name, ".")
            local ext = parts[#parts] or ""
            if ext == "lua" then
                total = total + 1
                local ok, err = pcall(function()
                    local src = std.fs.read(entry.path)
                    -- Basic sanity: must start with valid Lua
                    return src and #src > 0
                end)
                if not (ok and err) then
                    failed = failed + 1
                    print("FAIL: " .. entry.path)
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
    -- Default: check scripts/lua directory
    return check_dir("scripts/lua")
end
