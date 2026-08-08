-- build_identity.lua entry point — freeze truthful source inputs into a batch-importable build environment.
-- Args: REPO PROFILE OUTPUT_PATH
-- The build_identity module is loaded via dofile for standalone CLI execution.

-- Load the library module (works with pcall for graceful fallback)
local ok, _ = pcall(function()
    dofile("scripts/lua/lib/build_identity.lua")
end)
if not ok or build_identity == nil then
    -- Fallback: try relative to script location
    local script_dir = "scripts/lua/lib"
    local lib_path = string_split(script_dir, "/")
    -- Use std.path.join if available
    local lib = std.path.join(script_dir, "build_identity.lua")
    local src, err = pcall(function() return std.fs.read_to_string(lib) end)
    if src and type(src) == "string" then
        local fn, load_err = load(src)
        if fn then
            fn()
        end
    end
end

local n = __host.args_len()
if n ~= 3 then
    error("expected: REPO PROFILE OUTPUT_PATH")
end

local repo = __host.arg(0)
local profile = __host.arg(1)
local output_path = __host.arg(2)

return build_identity.write(repo, profile, output_path)
