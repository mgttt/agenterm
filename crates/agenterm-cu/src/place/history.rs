//! Per-application undo/redo stack, persisted across `cu` invocations.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
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

#[derive(Clone)]
pub struct PlaceHistory {
    path: PathBuf,
    store: Store,
}

#[derive(Debug)]
pub struct HistorySaveError {
    message: String,
    published: bool,
}

impl HistorySaveError {
    fn before_publish(message: String) -> Self {
        Self {
            message,
            published: false,
        }
    }

    fn after_publish(message: String) -> Self {
        Self {
            message,
            published: true,
        }
    }

    pub fn published(&self) -> bool {
        self.published
    }
}

impl std::fmt::Display for HistorySaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl PlaceHistory {
    pub fn open() -> Result<Self, String> {
        Self::open_at(history_path())
    }

    pub(crate) fn open_at(path: PathBuf) -> Result<Self, String> {
        let store = if path.exists() {
            let raw = fs::read_to_string(&path)
                .map_err(|error| format!("read {}: {error}", path.display()))?;
            let store: Store = serde_json::from_str(&raw)
                .map_err(|error| format!("parse {}: {error}", path.display()))?;
            validate_store(&store)
                .map_err(|error| format!("validate {}: {error}", path.display()))?;
            store
        } else {
            Store::default()
        };
        Ok(Self { path, store })
    }

    fn record(&mut self, app_key: &str, handle: isize, before: Rect, after: Rect) {
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

    pub fn plan_record(&self, app_key: &str, handle: isize, before: Rect, after: Rect) -> Self {
        let mut planned = self.clone();
        planned.record(app_key, handle, before, after);
        planned
    }

    pub fn plan_undo(&self, app_key: &str) -> Option<(Self, Rect)> {
        let mut planned = self.clone();
        let (_, rect) = planned.undo(app_key)?;
        Some((planned, rect))
    }

    pub fn plan_redo(&self, app_key: &str) -> Option<(Self, Rect)> {
        let mut planned = self.clone();
        let (_, rect) = planned.redo(app_key)?;
        Some((planned, rect))
    }

    fn undo(&mut self, app_key: &str) -> Option<(isize, Rect)> {
        let stack = self.store.apps.get_mut(app_key)?;
        if stack.index == 0 || stack.items.is_empty() {
            return None;
        }
        stack.index -= 1;
        Some(from_item(&stack.items[stack.index]))
    }

    fn redo(&mut self, app_key: &str) -> Option<(isize, Rect)> {
        let stack = self.store.apps.get_mut(app_key)?;
        if stack.index + 1 >= stack.items.len() {
            return None;
        }
        stack.index += 1;
        Some(from_item(&stack.items[stack.index]))
    }

    pub fn save(&self) -> Result<(), HistorySaveError> {
        self.save_with(write_prepared, publish_prepared)
    }

    fn save_with<W, F>(&self, write: W, publish: F) -> Result<(), HistorySaveError>
    where
        W: FnOnce(&mut File, &[u8]) -> io::Result<()>,
        F: FnOnce(&Path, &Path) -> io::Result<()>,
    {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| {
            HistorySaveError::before_publish(format!("create {}: {error}", parent.display()))
        })?;
        let raw = serde_json::to_string_pretty(&self.store)
            .map_err(|error| HistorySaveError::before_publish(error.to_string()))?;
        let (temporary, mut file) = create_temporary(parent, &self.path).map_err(|error| {
            HistorySaveError::before_publish(format!("prepare {}: {error}", self.path.display()))
        })?;
        let mut cleanup = TemporaryFile::new(temporary.clone());
        write(&mut file, raw.as_bytes()).map_err(|error| {
            HistorySaveError::before_publish(format!("write {}: {error}", temporary.display()))
        })?;
        drop(file);
        publish(&temporary, &self.path).map_err(|error| {
            HistorySaveError::before_publish(format!("publish {}: {error}", self.path.display()))
        })?;
        cleanup.disarm();
        sync_parent(parent).map_err(|error| {
            HistorySaveError::after_publish(format!("sync {}: {error}", parent.display()))
        })
    }
}

fn write_prepared(file: &mut File, raw: &[u8]) -> io::Result<()> {
    file.write_all(raw)?;
    file.flush()?;
    file.sync_all()
}

fn validate_store(store: &Store) -> Result<(), String> {
    for (app, stack) in &store.apps {
        if stack.items.len() > 40 {
            return Err(format!(
                "application {app:?} has {} items; maximum is 40",
                stack.items.len()
            ));
        }
        if stack.items.is_empty() {
            if stack.index != 0 {
                return Err(format!(
                    "application {app:?} has index {} for an empty stack",
                    stack.index
                ));
            }
        } else if stack.index >= stack.items.len() {
            return Err(format!(
                "application {app:?} has index {} outside {} items",
                stack.index,
                stack.items.len()
            ));
        }
    }
    Ok(())
}

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn create_temporary(parent: &Path, destination: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "history file name required"))?;
    for _ in 0..32 {
        let temporary = parent.join(format!(
            ".{}.{}-{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "history temporary name attempts exhausted",
    ))
}

fn publish_prepared(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> io::Result<()> {
    Ok(())
}

struct TemporaryFile {
    path: PathBuf,
    armed: bool,
}

impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "agenterm-cu-history-{}-{}",
                std::process::id(),
                NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create isolated history fixture");
            Self(path)
        }

        fn history(&self) -> PathBuf {
            self.0.join("history.json")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rect(value: usize) -> Rect {
        Rect::new(value as f64, value as f64 + 1.0, 640.0, 480.0)
    }

    #[test]
    fn record_truncates_redo_and_preserves_undo_redo_boundaries() {
        let directory = TestDirectory::new();
        let mut history = PlaceHistory::open_at(directory.history()).expect("open history");
        history.record("editor", 7, rect(0), rect(1));
        history.record("editor", 7, rect(1), rect(2));

        assert_eq!(history.undo("editor"), Some((7, rect(1))));
        history.record("editor", 7, rect(1), rect(3));
        assert_eq!(history.redo("editor"), None, "new record drops redo branch");
        assert_eq!(history.undo("editor"), Some((7, rect(1))));
        assert_eq!(history.undo("editor"), Some((7, rect(0))));
        assert_eq!(history.undo("editor"), None);
        assert_eq!(history.redo("editor"), Some((7, rect(1))));
        assert_eq!(history.redo("editor"), Some((7, rect(3))));
        assert_eq!(history.redo("editor"), None);
    }

    #[test]
    fn undo_plan_advances_only_the_cloned_history() {
        let directory = TestDirectory::new();
        let history_path = directory.history();
        let mut history = PlaceHistory::open_at(history_path).expect("open history");
        history.record("editor", 7, rect(0), rect(1));

        let (planned, target) = history.plan_undo("editor").expect("undo plan");
        assert_eq!(target, rect(0));
        assert!(history.plan_redo("editor").is_none());
        assert_eq!(
            history.plan_undo("editor").map(|(_, rect)| rect),
            Some(rect(0)),
            "planning must not advance the source cursor"
        );
        assert_eq!(
            planned.plan_redo("editor").map(|(_, rect)| rect),
            Some(rect(1)),
            "the cloned plan owns the advanced cursor"
        );
    }

    #[test]
    fn record_caps_each_application_at_forty_items() {
        let directory = TestDirectory::new();
        let mut history = PlaceHistory::open_at(directory.history()).expect("open history");
        for value in 1..=45 {
            history.record("editor", 9, rect(value - 1), rect(value));
        }

        for expected in (7..=44).rev() {
            assert_eq!(history.undo("editor"), Some((9, rect(expected))));
        }
        assert_eq!(history.undo("editor"), Some((9, rect(6))));
        assert_eq!(
            history.undo("editor"),
            None,
            "oldest six states were evicted"
        );
    }

    #[test]
    fn malformed_existing_history_is_reported_without_rewriting_it() {
        let directory = TestDirectory::new();
        let path = directory.history();
        let malformed = br#"{"apps":{"editor":{"items":[],"index":1}}}"#;
        fs::write(&path, malformed).expect("seed malformed history");

        let error = PlaceHistory::open_at(path.clone())
            .err()
            .expect("invalid cursor must fail honestly");
        assert!(error.contains("empty stack"), "unexpected error: {error}");
        assert_eq!(fs::read(path).expect("history remains readable"), malformed);

        let syntax_path = directory.0.join("syntax.json");
        let invalid_json = b"{not-json";
        fs::write(&syntax_path, invalid_json).expect("seed invalid JSON");
        let error = PlaceHistory::open_at(syntax_path.clone())
            .err()
            .expect("invalid JSON must fail honestly");
        assert!(error.contains("parse"), "unexpected error: {error}");
        assert_eq!(
            fs::read(syntax_path).expect("invalid JSON remains readable"),
            invalid_json
        );
    }

    #[test]
    fn failed_publication_retains_existing_valid_history() {
        let directory = TestDirectory::new();
        let path = directory.history();
        let mut history = PlaceHistory::open_at(path.clone()).expect("open history");
        history.record("editor", 11, rect(0), rect(1));
        history.save().expect("seed valid history");
        let valid = fs::read(&path).expect("read valid history");

        history.record("editor", 11, rect(1), rect(2));
        let error = history
            .save_with(write_prepared, |_source, _destination| {
                Err(io::Error::other("injected publication failure"))
            })
            .expect_err("injected publication failure must surface");
        assert!(error.to_string().contains("injected publication failure"));
        assert!(!error.published());
        assert_eq!(fs::read(&path).expect("read retained history"), valid);
        assert_eq!(
            fs::read_dir(&directory.0).expect("list fixture").count(),
            1,
            "failed staging file must be reclaimed"
        );
    }

    #[test]
    fn failed_partial_write_retains_existing_valid_history() {
        let directory = TestDirectory::new();
        let path = directory.history();
        let mut history = PlaceHistory::open_at(path.clone()).expect("open history");
        history.record("editor", 12, rect(0), rect(1));
        history.save().expect("seed valid history");
        let valid = fs::read(&path).expect("read valid history");

        history.record("editor", 12, rect(1), rect(2));
        let error = history
            .save_with(
                |file, raw| {
                    file.write_all(&raw[..raw.len() / 2])?;
                    Err(io::Error::other("injected partial write failure"))
                },
                publish_prepared,
            )
            .expect_err("injected write failure must surface");
        assert!(error.to_string().contains("injected partial write failure"));
        assert!(!error.published());
        assert_eq!(fs::read(&path).expect("read retained history"), valid);
        assert_eq!(
            fs::read_dir(&directory.0).expect("list fixture").count(),
            1,
            "partial staging file must be reclaimed"
        );
    }

    #[test]
    fn successful_save_reopens_with_the_same_cursor_and_items() {
        let directory = TestDirectory::new();
        let path = directory.history();
        let mut history = PlaceHistory::open_at(path.clone()).expect("open history");
        history.record("editor", 13, rect(0), rect(1));
        history.save().expect("save initial history");
        history.record("editor", 13, rect(1), rect(2));
        assert_eq!(history.undo("editor"), Some((13, rect(1))));
        history.save().expect("replace history");

        let mut reopened = PlaceHistory::open_at(path).expect("reopen history");
        assert_eq!(reopened.redo("editor"), Some((13, rect(2))));
        assert_eq!(reopened.redo("editor"), None);
        assert_eq!(reopened.undo("editor"), Some((13, rect(1))));
    }
}
