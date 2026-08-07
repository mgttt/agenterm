fn is_artifact_name(name) {
    if name == "agenterm.exe" {
        return true;
    }
    if !name.starts_with("agenterm-") || !name.ends_with(".exe") {
        return false;
    }
    if name.contains("/") || name.contains("\\") {
        return false;
    }
    true
}
