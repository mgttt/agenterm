//! Linux AT-SPI2 accessibility tree and node actuation.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use atspi::connection::AccessibilityConnection;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{CoordType, ObjectRefOwned, StateSet};
use tokio::time::{Duration, timeout};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::zvariant::OwnedObjectPath;

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNode, AccessibilityNodeAction, AccessibilityTree,
    AccessibilityTreeError,
};

const MAX_NODES: usize = 1_000;
const MAX_DEPTH: u32 = 32;
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);
const NULL_OBJECT_PATH: &str = "/org/a11y/atspi/null";
const APPLICATION_ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";

/// AT-SPI object on the a11y bus. Destination may be a unique name (`:1.47`)
/// or a well-known name (WebKit's `org.webkit.app-*.Sandboxed.WebProcess-*`).
/// The atspi `ObjectRef` type only accepts unique names, so embeds that use a
/// well-known destination must be carried as raw `(name, path)` pairs.
#[derive(Clone, Debug, Eq, PartialEq)]
struct BusObject {
    dest: String,
    path: String,
}

#[derive(Clone, Debug)]
struct WindowIdentity {
    handle: isize,
    pid: Option<u32>,
    descendant_pids: HashSet<u32>,
    title: String,
    wm_class: Vec<String>,
    comm: String,
    bounds: AccessibilityBounds,
}

static RUNTIME: OnceLock<&'static tokio::runtime::Runtime> = OnceLock::new();
static SHARED_CONNECTION: OnceLock<Mutex<Option<AccessibilityConnection>>> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        // Leak: dropping this runtime at process exit aborts in-flight zbus
        // tasks and can take the a11y bus / Chrome's AT-SPI bridge with it.
        Box::leak(Box::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for AT-SPI"),
        ))
    })
}

fn shared_connection_slot() -> &'static Mutex<Option<AccessibilityConnection>> {
    SHARED_CONNECTION.get_or_init(|| Mutex::new(None))
}

fn cached_connection() -> Option<AccessibilityConnection> {
    shared_connection_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn remember_connection(conn: AccessibilityConnection) -> AccessibilityConnection {
    let mut slot = shared_connection_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = slot.as_ref() {
        return existing.clone();
    }
    // Leak one owner so process teardown cannot Drop the zbus connection
    // (that Drop talks to the a11y bus while the runtime is dying).
    let leaked: &'static AccessibilityConnection = Box::leak(Box::new(conn.clone()));
    *slot = Some(leaked.clone());
    conn
}

pub(crate) fn capability_status() -> CapabilityStatus {
    match runtime().block_on(connect()) {
        Ok(_) => CapabilityStatus::Available,
        Err(AccessibilityTreeError::Unsupported { reason }) => {
            CapabilityStatus::Unsupported { reason }
        }
        Err(AccessibilityTreeError::Failed { code, message }) => {
            CapabilityStatus::Failed { code, message }
        }
    }
}

pub(crate) fn tree_for_window(
    window_handle: Option<isize>,
) -> Result<AccessibilityTree, AccessibilityTreeError> {
    runtime().block_on(async {
        timeout(
            SNAPSHOT_TIMEOUT,
            tree_for_window_async(window_handle, MAX_NODES, MAX_DEPTH),
        )
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_tree_timeout",
                "AT-SPI tree snapshot exceeded its deadline",
            )
        })?
    })
}

/// Keep the shared a11y-bus connection pumping until the toolkit finishes
/// emitting events from the last keystroke. Exiting immediately after XTest
/// closes the socket under those events and Chrome's renderer tree dies.
pub(crate) fn drain_bus() {
    if cached_connection().is_none() {
        return;
    }
    runtime().block_on(async {
        tokio::time::sleep(Duration::from_millis(400)).await;
    });
}

pub(crate) fn perform_node_action(
    window_handle: Option<isize>,
    node_id: &str,
    action: AccessibilityNodeAction,
) -> Result<(), AccessibilityTreeError> {
    runtime().block_on(async {
        timeout(
            SNAPSHOT_TIMEOUT,
            perform_node_action_async(window_handle, node_id, action),
        )
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_action_timeout",
                "AT-SPI node action exceeded its deadline",
            )
        })?
    })
}

async fn tree_for_window_async(
    window_handle: Option<isize>,
    max_nodes: usize,
    max_depth: u32,
) -> Result<AccessibilityTree, AccessibilityTreeError> {
    let conn = connect().await?;
    let identity = window_handle.and_then(window_identity);
    let roots = registry_children(&conn).await?;
    let selected = select_roots(&conn, roots, identity.as_ref()).await?;
    if selected.is_empty() {
        if let Some(identity) = identity.as_ref() {
            return Ok(window_frame_tree(identity));
        }
        return Err(AccessibilityTreeError::failed(
            "a11y_tree_empty",
            "no AT-SPI application roots matched the requested window",
        ));
    }

    let mut nodes = Vec::new();
    let mut queue: VecDeque<(BusObject, String, Option<String>, u32)> = VecDeque::new();
    for (index, object) in selected.into_iter().enumerate() {
        queue.push_back((object, format!("/{index}"), None, 0));
    }

    while let Some((object, id, parent_id, depth)) = queue.pop_front() {
        if nodes.len() >= max_nodes {
            break;
        }
        let Ok(proxy) = open_bus_object(&conn, &object).await else {
            continue;
        };
        let node = read_node(&proxy, id.clone(), parent_id.clone()).await;
        let child_budget = max_nodes.saturating_sub(nodes.len() + queue.len());
        let child_refs = if depth < max_depth && child_budget > 0 {
            raw_children(&proxy, child_budget).await.unwrap_or_default()
        } else {
            Vec::new()
        };
        nodes.push(node);
        for (child_index, child) in child_refs.into_iter().enumerate() {
            let child_id = format!("{id}/{child_index}");
            queue.push_back((child, child_id, Some(id.clone()), depth + 1));
        }
    }

    if nodes.is_empty() {
        if let Some(identity) = identity.as_ref() {
            return Ok(window_frame_tree(identity));
        }
        return Err(AccessibilityTreeError::failed(
            "a11y_tree_empty",
            "no AT-SPI application roots matched the requested window",
        ));
    }

    let root_id = nodes
        .first()
        .map(|node| node.id.clone())
        .unwrap_or_else(|| "/0".to_owned());

    Ok(AccessibilityTree {
        backend: "at-spi2",
        window_handle,
        root_id,
        nodes,
    })
}

async fn perform_node_action_async(
    window_handle: Option<isize>,
    node_id: &str,
    action: AccessibilityNodeAction,
) -> Result<(), AccessibilityTreeError> {
    let indices = parse_node_path(node_id)?;
    let conn = connect().await?;
    let identity = window_handle.and_then(window_identity);
    let roots = registry_children(&conn).await?;
    let selected = select_roots(&conn, roots, identity.as_ref()).await?;
    if selected.is_empty() {
        return activate_window_node(window_handle, node_id);
    }
    let object = resolve_path(&conn, &selected, &indices).await?;
    let proxy = open_bus_object(&conn, &object).await?;
    match action {
        AccessibilityNodeAction::Click => invoke_click_action(&proxy).await,
        AccessibilityNodeAction::Focus => {
            if node_reports_focused(&proxy).await {
                return Ok(());
            }
            if invoke_named_action(&proxy, &["focus"]).await.is_ok() {
                wait_until_focused(&proxy).await;
                return Ok(());
            }
            let proxies = proxy.proxies().await.map_err(map_atspi_err)?;
            let component = proxies.component().await.map_err(map_atspi_err)?;
            component.grab_focus().await.map_err(map_atspi_err)?;
            wait_until_focused(&proxy).await;
            Ok(())
        }
    }
}

fn activate_window_node(
    window_handle: Option<isize>,
    node_id: &str,
) -> Result<(), AccessibilityTreeError> {
    if node_id != "/0" {
        return Err(AccessibilityTreeError::failed(
            "a11y_node_not_found",
            format!("node path {node_id} is unavailable"),
        ));
    }
    let handle = window_handle.ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_tree_empty",
            "no AT-SPI application roots matched the requested window",
        )
    })?;
    activate_x11_window(handle)
}

/// One AT-SPI bus connection per process.
///
/// `send-keys --name` snapshots the tree, focuses the node, then injects
/// XTest. Opening a fresh `AccessibilityConnection` for each of those steps
/// (and dropping it before the key event) tears Chrome's renderer tree down
/// and can crash the browser, so the next named command sees
/// `a11y_node_not_found` / `accessibility-tree mechanism unavailable`.
/// Clone the shared connection instead of dropping the a11y bus.
async fn connect() -> Result<AccessibilityConnection, AccessibilityTreeError> {
    hydrate_session_bus_env();
    if let Some(conn) = cached_connection() {
        return Ok(conn);
    }
    let conn = AccessibilityConnection::new().await.map_err(|error| {
        AccessibilityTreeError::failed("a11y_connect_failed", error.to_string())
    })?;
    Ok(remember_connection(conn))
}

fn hydrate_session_bus_env() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return;
    }
    if let Some(address) = dbus_address_from_process("at-spi2-registryd")
        .or_else(|| dbus_address_from_process("at-spi-bus-launcher"))
    {
        unsafe {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address);
        }
        return;
    }
    let uid = unsafe { libc::getuid() };
    let path = format!("/run/user/{uid}/bus");
    if std::path::Path::new(&path).exists() {
        unsafe {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={path}"));
        }
    }
}

fn process_cmdline_matches(cmdline: &[u8], process_name: &str) -> bool {
    let needle = process_name.as_bytes();
    cmdline.split(|byte| *byte == 0).any(|part| {
        part == needle
            || part.ends_with(needle)
            || part
                .rsplit(|byte| *byte == b'/')
                .next()
                .is_some_and(|base| base == needle)
    })
}

fn dbus_address_from_process(process_name: &str) -> Option<String> {
    let proc_root = std::fs::read_dir("/proc").ok()?;
    for entry in proc_root.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        if !process_cmdline_matches(&cmdline, process_name) {
            continue;
        }
        let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        if let Some(address) = dbus_address_from_environ(&environ) {
            return Some(address);
        }
    }
    None
}

#[cfg(test)]
fn is_usable_object_ref(object_ref: &ObjectRefOwned) -> bool {
    !object_ref.is_null()
}

async fn child_at_index_skip_null(
    proxy: &AccessibleProxy<'_>,
    index: i32,
) -> Result<Option<ObjectRefOwned>, AccessibilityTreeError> {
    match proxy.get_child_at_index(index).await {
        Ok(child) if child.is_null() => Ok(None),
        Ok(child) => Ok(Some(child)),
        Err(_) => Ok(None),
    }
}

async fn child_at_logical_index(
    proxy: &AccessibleProxy<'_>,
    logical_index: usize,
) -> Result<BusObject, AccessibilityTreeError> {
    let children = raw_children(proxy, usize::MAX).await?;
    children.into_iter().nth(logical_index).ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_node_not_found",
            format!("child index {logical_index} is unavailable"),
        )
    })
}

fn dbus_address_from_environ(environ: &[u8]) -> Option<String> {
    environ.split(|byte| *byte == 0).find_map(|item| {
        let text = std::str::from_utf8(item).ok()?;
        text.strip_prefix("DBUS_SESSION_BUS_ADDRESS=")
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

async fn registry_children(
    conn: &AccessibilityConnection,
) -> Result<Vec<ObjectRefOwned>, AccessibilityTreeError> {
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(map_atspi_err)?;
    let child_count = root.child_count().await.map_err(map_atspi_err)?;
    let limit = usize::try_from(child_count).unwrap_or(0).min(256);
    let mut children = Vec::new();
    for index in 0..limit {
        if let Ok(Some(child)) = child_at_index_skip_null(&root, index as i32).await {
            children.push(child);
        }
    }
    if children.is_empty() && child_count > 0 {
        return Err(AccessibilityTreeError::failed(
            "a11y_registry_read_failed",
            "AT-SPI registry reported children but none could be read",
        ));
    }
    Ok(children)
}

async fn select_roots(
    conn: &AccessibilityConnection,
    roots: Vec<ObjectRefOwned>,
    identity: Option<&WindowIdentity>,
) -> Result<Vec<BusObject>, AccessibilityTreeError> {
    let mut selected = Vec::new();
    let dbus = DBusProxy::new(conn.connection()).await.ok();
    for object_ref in roots {
        let Some(object) = bus_object_from_ref(&object_ref) else {
            continue;
        };
        let Some(identity) = identity else {
            selected.push(object);
            continue;
        };
        if root_matches_window(conn, dbus.as_ref(), &object_ref, &object, identity).await {
            selected.push(object);
        }
    }
    if let (Some(identity), Some(dbus)) = (identity, dbus.as_ref()) {
        let extras = extra_roots_for_window(conn, dbus, identity, &selected).await;
        selected.extend(extras);
    }
    Ok(selected)
}

async fn root_matches_window(
    conn: &AccessibilityConnection,
    dbus: Option<&DBusProxy<'_>>,
    object_ref: &ObjectRefOwned,
    object: &BusObject,
    identity: &WindowIdentity,
) -> bool {
    if let Some(pid) = object_ref_pid(dbus, object_ref).await
        && identity.owns_pid(pid)
    {
        return true;
    }
    let Ok(proxy) = open_bus_object(conn, object).await else {
        return false;
    };
    let name = proxy.name().await.unwrap_or_default();
    if identity.matches_app_name(&name) || identity.matches_title(&name) {
        return true;
    }
    let Ok(children) = raw_children(&proxy, 16).await else {
        return false;
    };
    for child in children {
        let Ok(child_proxy) = open_bus_object(conn, &child).await else {
            continue;
        };
        let child_name = child_proxy.name().await.unwrap_or_default();
        let role = role_name(&child_proxy).await.to_ascii_lowercase();
        if identity.matches_title(&child_name)
            && (role.contains("frame") || role.contains("window") || role.contains("application"))
        {
            return true;
        }
    }
    false
}

async fn extra_roots_for_window(
    conn: &AccessibilityConnection,
    dbus: &DBusProxy<'_>,
    identity: &WindowIdentity,
    already: &[BusObject],
) -> Vec<BusObject> {
    let Ok(names) = dbus.list_names().await else {
        return Vec::new();
    };
    let mut extra = Vec::new();
    for name in names {
        let dest = name.as_str();
        if dest == "org.freedesktop.DBus" || dest == "org.a11y.atspi.Registry" {
            continue;
        }
        let Ok(bus_name) = BusName::try_from(dest.to_owned()) else {
            continue;
        };
        let Ok(pid) = dbus.get_connection_unix_process_id(bus_name).await else {
            continue;
        };
        if !identity.owns_pid(pid) {
            continue;
        }
        if already
            .iter()
            .chain(extra.iter())
            .any(|root| root.dest == dest)
        {
            continue;
        }
        let candidate = BusObject {
            dest: dest.to_owned(),
            path: APPLICATION_ROOT_PATH.to_owned(),
        };
        if open_bus_object(conn, &candidate).await.is_ok() {
            extra.push(candidate);
        }
    }
    extra
}

async fn resolve_path(
    conn: &AccessibilityConnection,
    roots: &[BusObject],
    indices: &[usize],
) -> Result<BusObject, AccessibilityTreeError> {
    let Some((&root_index, rest)) = indices.split_first() else {
        return Err(AccessibilityTreeError::failed(
            "a11y_node_not_found",
            "node path is empty",
        ));
    };
    let mut current = roots.get(root_index).cloned().ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_node_not_found",
            format!("application root index {root_index} is out of range"),
        )
    })?;
    for &child_index in rest {
        let proxy = open_bus_object(conn, &current).await?;
        current = child_at_logical_index(&proxy, child_index).await?;
    }
    Ok(current)
}

async fn raw_children(
    proxy: &AccessibleProxy<'_>,
    limit: usize,
) -> Result<Vec<BusObject>, AccessibilityTreeError> {
    if let Ok(children) = raw_children_via_get_children(proxy).await
        && !children.is_empty()
    {
        return Ok(children.into_iter().take(limit).collect());
    }
    raw_children_via_index(proxy, limit).await
}

async fn raw_children_via_get_children(
    proxy: &AccessibleProxy<'_>,
) -> Result<Vec<BusObject>, AccessibilityTreeError> {
    let reply = proxy
        .inner()
        .call_method("GetChildren", &())
        .await
        .map_err(map_atspi_err)?;
    let pairs: Vec<(String, OwnedObjectPath)> =
        reply.body().deserialize().map_err(map_atspi_err)?;
    Ok(pairs
        .into_iter()
        .filter_map(|(dest, path)| bus_object_from_pair(dest, path.as_str()))
        .collect())
}

async fn raw_children_via_index(
    proxy: &AccessibleProxy<'_>,
    limit: usize,
) -> Result<Vec<BusObject>, AccessibilityTreeError> {
    let child_count = proxy.child_count().await.unwrap_or(0);
    let count = usize::try_from(child_count).unwrap_or(0);
    let mut children = Vec::new();
    for index in 0..count {
        if children.len() >= limit {
            break;
        }
        if let Some(child) = raw_child_at_index(proxy, index as i32).await? {
            children.push(child);
        }
    }
    Ok(children)
}

async fn raw_child_at_index(
    proxy: &AccessibleProxy<'_>,
    index: i32,
) -> Result<Option<BusObject>, AccessibilityTreeError> {
    let reply = match proxy.inner().call_method("GetChildAtIndex", &(index)).await {
        Ok(reply) => reply,
        Err(_) => return Ok(None),
    };
    let (dest, path): (String, OwnedObjectPath) = match reply.body().deserialize() {
        Ok(pair) => pair,
        Err(_) => return Ok(None),
    };
    Ok(bus_object_from_pair(dest, path.as_str()))
}

fn bus_object_from_pair(dest: String, path: &str) -> Option<BusObject> {
    if dest.is_empty() || path.is_empty() || path == NULL_OBJECT_PATH {
        return None;
    }
    Some(BusObject {
        dest,
        path: path.to_owned(),
    })
}

fn bus_object_from_ref(object_ref: &ObjectRefOwned) -> Option<BusObject> {
    bus_object_from_pair(
        object_ref.name_as_str()?.to_owned(),
        object_ref.path_as_str(),
    )
}

async fn open_bus_object<'a>(
    conn: &'a AccessibilityConnection,
    object: &BusObject,
) -> Result<AccessibleProxy<'a>, AccessibilityTreeError> {
    let dbus = DBusProxy::new(conn.connection()).await.ok();
    let dest = resolve_dest(dbus.as_ref(), &object.dest).await;
    let path = object.path.clone();
    AccessibleProxy::builder(conn.connection())
        .destination(dest)
        .map_err(map_atspi_err)?
        .path(path)
        .map_err(map_atspi_err)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn resolve_dest(dbus: Option<&DBusProxy<'_>>, dest: &str) -> String {
    if is_unique_bus_name(dest) {
        return dest.to_owned();
    }
    let Some(dbus) = dbus else {
        return dest.to_owned();
    };
    let Ok(bus_name) = BusName::try_from(dest.to_owned()) else {
        return dest.to_owned();
    };
    dbus.get_name_owner(bus_name)
        .await
        .map(|owner| owner.as_str().to_owned())
        .unwrap_or_else(|_| dest.to_owned())
}

async fn read_node(
    proxy: &AccessibleProxy<'_>,
    id: String,
    parent_id: Option<String>,
) -> AccessibilityNode {
    let role = role_name(proxy).await;
    let name = proxy.name().await.unwrap_or_default();
    let states = states_from_proxy(proxy).await;
    let bounds = bounds_from_proxy(proxy)
        .await
        .unwrap_or(AccessibilityBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
    let actions = actions_from_proxy(proxy).await;
    let text = text_from_proxy(proxy).await;
    AccessibilityNode {
        id,
        parent_id,
        role,
        name,
        states,
        bounds,
        actions,
        text,
    }
}

async fn role_name(proxy: &AccessibleProxy<'_>) -> String {
    if let Ok(role) = proxy.get_role_name().await
        && !role.trim().is_empty()
    {
        return role;
    }
    proxy
        .get_role()
        .await
        .map(|role| format!("{role:?}"))
        .unwrap_or_else(|_| "unknown".to_string())
}

async fn states_from_proxy(proxy: &AccessibleProxy<'_>) -> Vec<String> {
    proxy
        .get_state()
        .await
        .map(state_labels)
        .unwrap_or_default()
}

async fn node_reports_focused(proxy: &AccessibleProxy<'_>) -> bool {
    states_from_proxy(proxy)
        .await
        .iter()
        .any(|state| state == "focused")
}

async fn wait_until_focused(proxy: &AccessibleProxy<'_>) {
    for _ in 0..10 {
        if node_reports_focused(proxy).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn state_labels(state: StateSet) -> Vec<String> {
    state
        .iter()
        .map(|value| format!("{value:?}"))
        .map(|label| label.to_ascii_lowercase())
        .collect()
}

async fn bounds_from_proxy(proxy: &AccessibleProxy<'_>) -> Option<AccessibilityBounds> {
    let proxies = proxy.proxies().await.ok()?;
    let component = proxies.component().await.ok()?;
    let (x, y, width, height) = component.get_extents(CoordType::Screen).await.ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(AccessibilityBounds {
        x,
        y,
        width,
        height,
    })
}

async fn actions_from_proxy(proxy: &AccessibleProxy<'_>) -> Vec<String> {
    let Ok(proxies) = proxy.proxies().await else {
        return Vec::new();
    };
    let Ok(action_proxy) = proxies.action().await else {
        return Vec::new();
    };
    action_proxy
        .get_actions()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|action| action.name)
        .collect()
}

async fn text_from_proxy(proxy: &AccessibleProxy<'_>) -> Option<String> {
    let proxies = proxy.proxies().await.ok()?;
    let text = proxies.text().await.ok()?;
    let count = text.character_count().await.ok()?.clamp(0, 4096);
    if count == 0 {
        return None;
    }
    text.get_text(0, count)
        .await
        .ok()
        .filter(|value| !value.is_empty())
}

/// AT-SPI `GetActions` returns localized names. Toolkits such as Chrome often
/// leave those strings empty while still exposing a default action at index 0.
fn named_action_index(names: &[String], preferred_names: &[&str]) -> Option<usize> {
    let preferred = preferred_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    names.iter().position(|name| {
        let lowered = name.to_ascii_lowercase();
        !lowered.is_empty() && preferred.iter().any(|wanted| wanted == &lowered)
    })
}

/// Structured click prefers a named `click`/`press`, then the AT-SPI default
/// action (index 0) when the node exposes any Action entries.
fn click_action_index(names: &[String]) -> Option<usize> {
    named_action_index(names, &["click", "press"]).or((!names.is_empty()).then_some(0))
}

fn format_available_actions(names: &[String]) -> String {
    names
        .iter()
        .map(|name| {
            if name.is_empty() {
                "<unnamed>"
            } else {
                name.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

async fn action_names(
    action_proxy: &atspi::proxy::action::ActionProxy<'_>,
) -> Result<Vec<String>, AccessibilityTreeError> {
    let actions = action_proxy.get_actions().await.map_err(map_atspi_err)?;
    if !actions.is_empty() {
        return Ok(actions.into_iter().map(|action| action.name).collect());
    }
    let n_actions = action_proxy.n_actions().await.unwrap_or(0).max(0);
    Ok(vec![String::new(); n_actions as usize])
}

async fn do_action_at(
    action_proxy: &atspi::proxy::action::ActionProxy<'_>,
    index: usize,
) -> Result<(), AccessibilityTreeError> {
    action_proxy
        .do_action(index as i32)
        .await
        .map_err(map_atspi_err)
        .map(|_| ())
}

async fn invoke_click_action(proxy: &AccessibleProxy<'_>) -> Result<(), AccessibilityTreeError> {
    let proxies = proxy.proxies().await.map_err(map_atspi_err)?;
    let action_proxy = proxies.action().await.map_err(|_| {
        AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            "node does not expose the AT-SPI Action interface",
        )
    })?;
    let names = action_names(&action_proxy).await?;
    let action_index = click_action_index(&names).ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            format!(
                "node exposes no AT-SPI actions to invoke; available: {}",
                format_available_actions(&names)
            ),
        )
    })?;
    do_action_at(&action_proxy, action_index).await
}

async fn invoke_named_action(
    proxy: &AccessibleProxy<'_>,
    preferred_names: &[&str],
) -> Result<(), AccessibilityTreeError> {
    let proxies = proxy.proxies().await.map_err(map_atspi_err)?;
    let action_proxy = proxies.action().await.map_err(|_| {
        AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            "node does not expose the AT-SPI Action interface",
        )
    })?;
    let names = action_names(&action_proxy).await?;
    let action_index = named_action_index(&names, preferred_names).ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            format!(
                "node exposes no requested AT-SPI actions; available: {}",
                format_available_actions(&names)
            ),
        )
    })?;
    do_action_at(&action_proxy, action_index).await
}

async fn object_ref_pid(dbus: Option<&DBusProxy<'_>>, object_ref: &ObjectRefOwned) -> Option<u32> {
    let dbus = dbus?;
    let bus_name = BusName::try_from(object_ref.name_as_str()?.to_string()).ok()?;
    dbus.get_connection_unix_process_id(bus_name).await.ok()
}

fn parse_node_path(node_id: &str) -> Result<Vec<usize>, AccessibilityTreeError> {
    if !node_id.starts_with('/') {
        return Err(AccessibilityTreeError::failed(
            "a11y_invalid_node_id",
            "node id must be a slash-separated path starting at the application root",
        ));
    }
    let mut indices = Vec::new();
    for segment in node_id.split('/').filter(|segment| !segment.is_empty()) {
        let index = segment.parse::<usize>().map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_invalid_node_id",
                format!("node path segment '{segment}' is not a child index"),
            )
        })?;
        indices.push(index);
    }
    if indices.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_invalid_node_id",
            "node id must include at least one application-root index",
        ));
    }
    Ok(indices)
}

impl WindowIdentity {
    fn owns_pid(&self, pid: u32) -> bool {
        self.pid == Some(pid) || self.descendant_pids.contains(&pid)
    }

    fn matches_app_name(&self, name: &str) -> bool {
        names_match_app(name, &self.wm_class, &self.comm)
    }

    fn matches_title(&self, name: &str) -> bool {
        titles_equivalent(&self.title, name)
    }
}

fn window_identity(window_handle: isize) -> Option<WindowIdentity> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    if window_handle == 0 {
        return None;
    }
    let window = u32::try_from(window_handle).ok()?;
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let pid_atom = connection
        .intern_atom(false, b"_NET_WM_PID")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let name_atom = connection
        .intern_atom(false, b"_NET_WM_NAME")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let utf8_atom = connection
        .intern_atom(false, b"UTF8_STRING")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let pid = connection
        .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32()?.next());
    let title = window_title_from_connection(&connection, window, name_atom, utf8_atom);
    let wm_class = window_class_from_connection(&connection, window);
    let comm = pid.map(process_comm).unwrap_or_default();
    let descendant_pids = pid.map(descendant_pids).unwrap_or_default();
    let bounds =
        window_bounds_from_connection(&connection, root, window).unwrap_or(AccessibilityBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
    Some(WindowIdentity {
        handle: window_handle,
        pid,
        descendant_pids,
        title,
        wm_class,
        comm,
        bounds,
    })
}

fn window_title_from_connection(
    connection: &x11rb::rust_connection::RustConnection,
    window: u32,
    name_atom: u32,
    utf8_atom: u32,
) -> String {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    if let Ok(cookie) = connection.get_property(false, window, name_atom, utf8_atom, 0, 16_384)
        && let Ok(reply) = cookie.reply()
        && reply.format == 8
        && reply.type_ == utf8_atom
    {
        let title = String::from_utf8_lossy(&reply.value).into_owned();
        if !title.is_empty() {
            return title;
        }
    }
    connection
        .get_property(
            false,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            0,
            16_384,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .filter(|reply| reply.format == 8)
        .map(|reply| String::from_utf8_lossy(&reply.value).into_owned())
        .unwrap_or_default()
}

fn window_class_from_connection(
    connection: &x11rb::rust_connection::RustConnection,
    window: u32,
) -> Vec<String> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    connection
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| parse_wm_class(&reply.value))
        .unwrap_or_default()
}

fn window_bounds_from_connection(
    connection: &x11rb::rust_connection::RustConnection,
    root: u32,
    window: u32,
) -> Option<AccessibilityBounds> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    let geometry = connection.get_geometry(window).ok()?.reply().ok()?;
    let translated = connection
        .translate_coordinates(window, root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    Some(AccessibilityBounds {
        x: i32::from(translated.dst_x),
        y: i32::from(translated.dst_y),
        width: i32::from(geometry.width),
        height: i32::from(geometry.height),
    })
}

fn window_frame_tree(identity: &WindowIdentity) -> AccessibilityTree {
    AccessibilityTree {
        backend: "at-spi2",
        window_handle: Some(identity.handle),
        root_id: "/0".to_owned(),
        nodes: vec![AccessibilityNode {
            id: "/0".to_owned(),
            parent_id: None,
            role: "frame".to_owned(),
            name: identity.title.clone(),
            states: vec![
                "enabled".to_owned(),
                "focusable".to_owned(),
                "showing".to_owned(),
                "visible".to_owned(),
            ],
            bounds: identity.bounds,
            actions: vec!["focus".to_owned(), "click".to_owned()],
            text: None,
        }],
    }
}

fn activate_x11_window(handle: isize) -> Result<(), AccessibilityTreeError> {
    use x11rb::CURRENT_TIME;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask, InputFocus};

    let window = u32::try_from(handle).map_err(|_| {
        AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            "window handle is not a valid XID",
        )
    })?;
    let (connection, screen) = x11rb::connect(None).map_err(|error| {
        AccessibilityTreeError::failed("a11y_backend_failed", error.to_string())
    })?;
    let root = connection
        .setup()
        .roots
        .get(screen)
        .map(|item| item.root)
        .ok_or_else(|| {
            AccessibilityTreeError::failed(
                "a11y_backend_failed",
                "configured X11 screen is missing",
            )
        })?;
    let atom = connection
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|error| AccessibilityTreeError::failed("a11y_backend_failed", error.to_string()))?
        .reply()
        .map_err(|error| AccessibilityTreeError::failed("a11y_backend_failed", error.to_string()))?
        .atom;
    let event = ClientMessageEvent::new(32, window, atom, [1, CURRENT_TIME, 0, 0, 0]);
    connection
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|error| {
            AccessibilityTreeError::failed("a11y_backend_failed", error.to_string())
        })?;
    let _ = connection.set_input_focus(InputFocus::POINTER_ROOT, window, CURRENT_TIME);
    connection.flush().map_err(|error| {
        AccessibilityTreeError::failed("a11y_backend_failed", error.to_string())
    })?;
    Ok(())
}

fn process_comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|name| name.trim().to_owned())
        .unwrap_or_default()
}

fn descendant_pids(root: u32) -> HashSet<u32> {
    let mut parents = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return HashSet::new();
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        if let Some(ppid) = parse_status_ppid(&status) {
            parents.push((pid, ppid));
        }
    }
    descendant_pids_from_parents(&parents, root)
}

fn parse_status_ppid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        line.strip_prefix("PPid:")
            .and_then(|rest| rest.trim().parse().ok())
    })
}

fn descendant_pids_from_parents(parents: &[(u32, u32)], root: u32) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(pid, ppid) in parents {
        if pid != root {
            children.entry(ppid).or_default().push(pid);
        }
    }
    let mut out = HashSet::new();
    let mut stack = children.get(&root).cloned().unwrap_or_default();
    while let Some(pid) = stack.pop() {
        if out.insert(pid)
            && let Some(kids) = children.get(&pid)
        {
            stack.extend(kids.iter().copied());
        }
    }
    out
}

fn parse_wm_class(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

fn normalize_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn names_match_app(app_name: &str, wm_class: &[String], comm: &str) -> bool {
    let app = normalize_name(app_name);
    if app.is_empty() {
        return false;
    }
    wm_class.iter().any(|class| normalize_name(class) == app)
        || (!comm.is_empty() && normalize_name(comm) == app)
}

fn titles_equivalent(window_title: &str, node_name: &str) -> bool {
    let left = normalize_name(window_title);
    let right = normalize_name(node_name);
    !left.is_empty() && left == right
}

fn is_unique_bus_name(name: &str) -> bool {
    name.starts_with(':')
}

fn map_atspi_err(error: impl std::fmt::Display) -> AccessibilityTreeError {
    let message = error.to_string();
    if message.contains("null reference") {
        return AccessibilityTreeError::failed("a11y_node_not_found", message);
    }
    AccessibilityTreeError::failed("a11y_backend_failed", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_paths_parse_as_child_indices() {
        assert_eq!(parse_node_path("/0").unwrap(), vec![0]);
        assert_eq!(parse_node_path("/0/2/5").unwrap(), vec![0, 2, 5]);
        assert!(parse_node_path("0/2").is_err());
    }

    #[test]
    fn null_atspi_object_refs_are_not_usable() {
        assert!(!is_usable_object_ref(&ObjectRefOwned::default()));
        assert!(is_usable_object_ref(
            &ObjectRefOwned::from_static_str_unchecked(":1.1", "/org/a11y/atspi/accessible/1")
        ));
    }

    #[test]
    fn parses_dbus_session_from_registry_environment() {
        assert_eq!(
            dbus_address_from_environ(
                b"LANG=C\0DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus\0"
            )
            .as_deref(),
            Some("unix:path=/run/user/1000/bus")
        );
        assert_eq!(
            dbus_address_from_environ(b"DBUS_SESSION_BUS_ADDRESS=\0"),
            None
        );
        assert_eq!(
            dbus_address_from_environ(b"NOT_DBUS_SESSION_BUS_ADDRESS=value\0\xff\0"),
            None
        );
    }

    #[test]
    fn named_action_match_is_case_insensitive_and_ignores_empty_names() {
        assert_eq!(
            named_action_index(&["Focus".into(), "Click".into()], &["click", "press"]),
            Some(1)
        );
        assert_eq!(
            named_action_index(&[String::new(), String::new()], &["click", "press"]),
            None
        );
        assert_eq!(
            named_action_index(&["focus".into()], &["click", "press"]),
            None
        );
    }

    #[test]
    fn click_uses_named_action_then_atspi_default_index() {
        assert_eq!(click_action_index(&["press".into()]), Some(0));
        assert_eq!(
            click_action_index(&["focus".into(), "Click".into()]),
            Some(1)
        );
        assert_eq!(click_action_index(&[String::new(), String::new()]), Some(0));
        assert_eq!(click_action_index(&[]), None);
    }

    #[test]
    fn available_action_list_marks_empty_names() {
        assert_eq!(
            format_available_actions(&[String::new(), String::new()]),
            "<unnamed>, <unnamed>"
        );
        assert_eq!(format_available_actions(&["click".into()]), "click");
    }

    #[test]
    fn hydrates_missing_dbus_session_address() {
        let prior = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        unsafe {
            std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
        }
        hydrate_session_bus_env();
        let hydrated = std::env::var("DBUS_SESSION_BUS_ADDRESS");
        if let Some(value) = prior {
            unsafe {
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", value);
            }
        }
        assert!(
            hydrated.is_ok(),
            "hydrate should discover a session bus from the running AT-SPI registry"
        );
    }

    #[test]
    fn unique_bus_names_start_with_colon() {
        assert!(is_unique_bus_name(":1.47"));
        assert!(!is_unique_bus_name(
            "org.webkit.app-deadbeef.Sandboxed.WebProcess-uuid"
        ));
    }

    #[test]
    fn well_known_embed_pairs_are_kept() {
        let child = bus_object_from_pair(
            "org.webkit.app-deadbeef.Sandboxed.WebProcess-uuid".into(),
            "/org/a11y/webkit/accessible/1",
        )
        .expect("well-known dest should not be dropped");
        assert_eq!(
            child.dest,
            "org.webkit.app-deadbeef.Sandboxed.WebProcess-uuid"
        );
        assert!(bus_object_from_pair(":1.1".into(), NULL_OBJECT_PATH).is_none());
        assert!(bus_object_from_pair(String::new(), "/org/a11y/atspi/accessible/1").is_none());
    }

    #[test]
    fn descendant_pid_walk_includes_nested_children() {
        let parents = [(20, 10), (21, 10), (30, 20), (40, 99)];
        let kids = descendant_pids_from_parents(&parents, 10);
        assert!(kids.contains(&20));
        assert!(kids.contains(&21));
        assert!(kids.contains(&30));
        assert!(!kids.contains(&40));
        assert!(!kids.contains(&10));
    }

    #[test]
    fn parses_proc_status_ppid() {
        assert_eq!(
            parse_status_ppid("Name:\tchrome\nPPid:\t205990\n"),
            Some(205990)
        );
        assert_eq!(parse_status_ppid("Name:\tinit\n"), None);
    }

    #[test]
    fn wm_class_and_comm_match_application_names() {
        assert!(names_match_app(
            "agenterm-con",
            &["agenterm-con".into(), "agenterm-con".into()],
            "agenterm-con"
        ));
        assert!(names_match_app(
            "Reasonix-desktop",
            &["reasonix-desktop".into(), "Reasonix-desktop".into()],
            "reasonix-deskto"
        ));
        assert!(!names_match_app(
            "Google Chrome",
            &["agenterm-con".into()],
            "agenterm-con"
        ));
    }

    #[test]
    fn window_title_match_is_exact_after_normalize() {
        assert!(titles_equivalent("Reasonix", "reasonix"));
        assert!(!titles_equivalent(
            "about:blank - Google Chrome",
            "Reasonix"
        ));
        assert!(!titles_equivalent("", ""));
    }

    #[test]
    fn parses_wm_class_double_string() {
        assert_eq!(
            parse_wm_class(b"agenterm-con\0agenterm-con\0"),
            vec!["agenterm-con", "agenterm-con"]
        );
    }
}
