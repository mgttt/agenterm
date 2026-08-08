-- timing-summary.lua — Read bootstrap timing data and output markdown summary.
-- Reads timing JSON from environment or file, outputs a markdown table.

local function read_timing()
    -- Try AGENTERM_BOOTSTRAP_TIMING_FILE env var
    local file = std.env.get("AGENTERM_BOOTSTRAP_TIMING_FILE")
    if file and std.fs.exists(file) then
        local data = std.json.parse_file(file)
        return data
    end
    -- Fallback: check for timing data in env vars
    local timing = {}
    local names = std.env.names()
    for _, name in ipairs(names) do
        if string_split(name, "AGENTERM_BOOTSTRAP_")[1] ~= name then
            local val = std.env.get(name)
            if val and #val > 0 then
                timing[name] = val
            end
        end
    end
    return timing
end

local function main()
    local data = read_timing()

    print("# Build Timing Summary")
    print("")
    if type(data) == "table" then
        -- Check if this is an object with keys or a simple table
        local count = 0
        for k, v in pairs(data) do
            count = count + 1
        end
        if count == 0 then
            print("_(no timing data available)_")
            return 0
        end
        print("| Stage | Value |")
        print("|-------|-------|")
        for k, v in pairs(data) do
            if type(v) == "number" then
                print(string.format("| %s | %d |", k, v))
            elseif type(v) == "string" then
                -- Try to parse as number for ms values
                local num = tonumber(v)
                if num then
                    print(string.format("| %s | %d |", k, num))
                else
                    print(string.format("| %s | %s |", k, v))
                end
            end
        end
    else
        print("_(no timing data available)_")
    end
    return 0
end

return main()
