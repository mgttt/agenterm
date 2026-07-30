// Run the public MCP executable conformance and isolation suite.
//
// Args:
//   [REPO]

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len <= 1, "expected: [REPO]");
let repo = if args.len == 1 {
    std::path::absolute(args[0]).display
} else {
    std::env::current_dir().display
};
let command = std::process::command("cargo");
command.args([
    "test", "--quiet", "--locked",
    "--test", "mcp_stdio", "--", "--nocapture"
]);
command.current_dir(repo);
command.env("AGENTERM_NO_ACTIVATE", "1");
command.timeout(std::time::Duration::from_secs(60));
let output = command.output();
require(output.success, output.error("mcp-conformance"));
let stdout = output.stdout_text();
if stdout != "" {
    print(stdout);
}
print(
    "PASS: public MCP lifecycle, same-source, wait, fault, privacy, and isolation"
);
