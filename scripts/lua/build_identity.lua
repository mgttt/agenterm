-- build_identity.lua entry point — freeze truthful source inputs into a batch-importable build environment.
-- Args: REPO PROFILE OUTPUT_PATH
-- The build_identity module is prepended at load time by the worker.

local n = __host.args_len()
if n ~= 3 then
    error("expected: REPO PROFILE OUTPUT_PATH")
end

local repo = __host.arg(0)
local profile = __host.arg(1)
local output_path = __host.arg(2)

return build_identity.write(repo, profile, output_path)
