//! Per-application undo/redo stack, persisted across `cu` invocations.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::geometry::Rect;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Item {
    handle: isize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AppStack {
    items: Vec<Item>,
    index: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Store {
    apps: BTreeMap<String, AppStack>,
}

pub struct PlaceHistory {
    path: PathBuf,
    store: Store,
}

impl PlaceHistory {
    pub fn open() -> Result<Self, String> {
        let path = history_path();
        let store = if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            Store::default()
        };
        Ok(Self { path, store })
    }

    pub fn record(&mut self, app_key: &str, handle: isize, before: Rect, after: Rect) {
        let stack = self.store.apps.entry(app_key.to_string()).or_default();
        if stack.items.is_empty() {
            stack.items.push(item(handle, before));
            stack.index = 0;
        } else if stack.index + 1 < stack.items.len() {
            stack.items.truncate(stack.index + 1);
        }
        stack.items.push(item(handle, after));
        stack.index = stack.items.len() - 1;
        while stack.items.len() > 40 {
            stack.items.remove(0);
            stack.index = stack.index.saturating_sub(1);
        }
    }

    pub fn undo(&mut self, app_key: &str) -> Option<(isize, Rect)> {
        let stack = self.store.apps.get_mut(app_key)?;
        if stack.index == 0 || stack.items.is_empty() {
            return None;
        }
        stack.index -= 1;
        Some(from_item(&stack.items[stack.index]))
    }

    pub fn redo(&mut self, app_key: &str) -> Option<(isize, Rect)> {
        let stack = self.store.apps.get_mut(app_key)?;
        if stack.index + 1 >= stack.items.len() {
            return None;
        }
        stack.index += 1;
        Some(from_item(&stack.items[stack.index]))
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(&self.store).map_err(|e| e.to_string())?;
        fs::write(&self.path, raw).map_err(|e| e.to_string())
    }
}

fn item(handle: isize, rect: Rect) -> Item {
    Item {
        handle,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

fn from_item(item: &Item) -> (isize, Rect) {
    (
        item.handle,
        Rect::new(item.x, item.y, item.width, item.height),
    )
}

fn history_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("AGENTERM_CU_PLACE_HISTORY") {
        return PathBuf::from(explicit);
    }
    home_data_dir().join("cu-place-history.json")
}

fn home_data_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return Path::new(&xdg).join("agenterm");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(".local/share/agenterm");
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return Path::new(&profile).join("AppData/Local/agenterm");
    }
    PathBuf::from("agenterm-cu-place-history")
}
