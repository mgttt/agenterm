-- stage-build.lua — Production task: copy executables from target/ to dist/.
-- Usage: stage-build.lua <repo-path> <dist-path>
-- Returns 0 on success, 1 on failure.

local function stage(repopath, distpath)
    local debug_dir = std.path.join(repopath, "target/debug")
    if not std.fs.exists(debug_dir) then
        print("stage: debug dir not found: " .. debug_dir)
        return 1
    end

    -- Ensure dist exists
    if not std.fs.exists(distpath) then
        std.fs.create_dir(distpath)
    end

    local entries = std.fs.read_dir(debug_dir)
    local copied = 0
    local failed = 0

    for _, entry in ipairs(entries) do
        if entry.is_file then
            local name = entry.name
            -- Only copy .exe and .dll files
            local parts = string_split(name, ".")
            local ext = parts[#parts] or ""
            if ext == "exe" or ext == "dll" or ext == "pdb" then
                local src = entry.path
                local dst = std.path.join(distpath, name)
                local ok, err = pcall(function()
                    std.fs.copy(src, dst)
                    -- Copy metadata for verification
                    local meta = std.fs.metadata(dst)
                    return meta and meta.is_file
                end)
                if ok and err then
                    copied = copied + 1
                    print("COPIED: " .. name)
                else
                    failed = failed + 1
                    local msg = "unknown error"
                    if type(err) == "string" then msg = err end
                    print("FAIL: " .. name .. " — " .. msg)
                end
            end
        end
    end

    print(string.format("stage: %d copied, %d failed", copied, failed))
    if failed > 0 then
        return 1
    end
    return 0
end

local n = __host.args_len()
if n == 2 then
    return stage(__host.arg(0), __host.arg(1))
else
    -- Default: use current dir
    local repopath = std.env.current_dir()
    local distpath = std.path.join(repopath, "dist")
    return stage(repopath, distpath)
end
