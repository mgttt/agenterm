//! Session-scoped named paste buffers (tmux/rmux-compatible control surface).
//!
//! Buffers live in the server process only (not on disk, not across servers).
//! They feed `paste-buffer` / scripting; they are **not** an agent mailbox
//! (see plan v0.1.15 M vs B′).

use std::collections::BTreeMap;

/// Default buffer name when `-b` is omitted (tmux-style first slot).
pub(crate) const DEFAULT_BUFFER_NAME: &str = "0";

/// Soft upper bound per buffer body (256 KiB).
pub(crate) const MAX_BUFFER_BYTES: usize = 256 * 1024;

/// Soft upper bound on concurrent named buffers.
pub(crate) const MAX_BUFFER_COUNT: usize = 32;

/// Soft upper bound on buffer name length (UTF-8 bytes).
pub(crate) const MAX_BUFFER_NAME_BYTES: usize = 64;

#[derive(Clone, Debug, Default)]
pub(crate) struct NamedBufferStore {
    buffers: BTreeMap<String, Vec<u8>>,
}

impl NamedBufferStore {
    pub(crate) fn new() -> Self {
        Self {
            buffers: BTreeMap::new(),
        }
    }

    pub(crate) fn resolve_name(name: Option<&str>) -> Result<String, String> {
        let name = name.unwrap_or(DEFAULT_BUFFER_NAME);
        if name.is_empty() {
            return Err("buffer name must not be empty".to_owned());
        }
        if name.len() > MAX_BUFFER_NAME_BYTES {
            return Err(format!(
                "buffer name must be at most {MAX_BUFFER_NAME_BYTES} bytes"
            ));
        }
        if name.chars().any(|ch| ch.is_control() || ch == '/' || ch == '\\') {
            return Err("buffer name must not contain control characters or path separators".to_owned());
        }
        Ok(name.to_owned())
    }

    pub(crate) fn set(&mut self, name: Option<&str>, data: Vec<u8>) -> Result<(), String> {
        let name = Self::resolve_name(name)?;
        if data.len() > MAX_BUFFER_BYTES {
            return Err(format!(
                "buffer body must be at most {MAX_BUFFER_BYTES} bytes (got {})",
                data.len()
            ));
        }
        if !self.buffers.contains_key(&name) && self.buffers.len() >= MAX_BUFFER_COUNT {
            return Err(format!(
                "at most {MAX_BUFFER_COUNT} named buffers may exist; delete one first"
            ));
        }
        self.buffers.insert(name, data);
        Ok(())
    }

    pub(crate) fn get(&self, name: Option<&str>) -> Result<&[u8], String> {
        let name = Self::resolve_name(name)?;
        self.buffers
            .get(&name)
            .map(Vec::as_slice)
            .ok_or_else(|| format!("can't find buffer: {name}"))
    }

    pub(crate) fn delete(&mut self, name: Option<&str>) -> Result<(), String> {
        let name = Self::resolve_name(name)?;
        if self.buffers.remove(&name).is_none() {
            return Err(format!("can't find buffer: {name}"));
        }
        Ok(())
    }

    pub(crate) fn list_lines(&self) -> String {
        if self.buffers.is_empty() {
            return String::new();
        }
        self.buffers
            .iter()
            .map(|(name, data)| format!("{name}: {} bytes", data.len()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_is_zero() {
        let mut store = NamedBufferStore::new();
        store.set(None, b"hi".to_vec()).unwrap();
        assert_eq!(store.get(None).unwrap(), b"hi");
        assert_eq!(store.get(Some("0")).unwrap(), b"hi");
    }

    #[test]
    fn rejects_oversized_body() {
        let mut store = NamedBufferStore::new();
        let err = store
            .set(Some("x"), vec![0u8; MAX_BUFFER_BYTES + 1])
            .unwrap_err();
        assert!(err.contains("at most"), "{err}");
    }

    #[test]
    fn delete_missing_fails() {
        let mut store = NamedBufferStore::new();
        assert!(store.delete(Some("nope")).is_err());
    }

    #[test]
    fn list_is_sorted_by_name() {
        let mut store = NamedBufferStore::new();
        store.set(Some("b"), b"xy".to_vec()).unwrap();
        store.set(Some("a"), b"z".to_vec()).unwrap();
        assert_eq!(store.list_lines(), "a: 1 bytes\nb: 2 bytes");
    }
}
