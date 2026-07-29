fn main() {
    std::process::exit(agenterm::run_mcp_entry_with_args(
        std::env::args().skip(1).collect(),
    ));
}
