//! Linux AT-SPI2 accessibility tree and node actuation.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

mod chrome_ax_set_value;

use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::action::ActionProxy;
use atspi::proxy::component::ComponentProxy;
use atspi::proxy::device_event_controller::{DeviceEvent, DeviceEventControllerProxy, EventType};
use atspi::proxy::device_event_listener::DeviceEventListenerProxy;
use atspi::proxy::editable_text::EditableTextProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::proxy::text::TextProxy;
use atspi::{CoordType, Interface, Role, StateSet};
use tokio::time::{Duration, timeout};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNode, AccessibilityNodeAction, AccessibilityTree,
    AccessibilityTreeError,
};

const MAX_NODES: usize = 1_000;
const MAX_DEPTH: u32 = 32;
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);
const NODE_TIMEOUT: Duration = Duration::from_millis(1500);
const ACTION_TIMEOUT: Duration = Duration::from_millis(250);
const NULL_OBJECT_PATH: &str = "/org/a11y/atspi/null";
const APPLICATION_ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const REGISTRY_DEST: &str = "org.a11y.atspi.Registry";
const A11Y_BUS_DEST: &str = "org.a11y.Bus";
const A11Y_BUS_PATH: &str = "/org/a11y/bus";
const A11Y_BUS_IFACE: &str = "org.a11y.Bus";

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
static SHARED_CONNECTION: OnceLock<Mutex<Option<zbus::Connection>>> = OnceLock::new();

thread_local! {
    static LAST_TEXT_VIA: Cell<&'static str> = const { Cell::new("editable-text") };
}

/// Last successful named text write route on this thread (`editable-text`
/// or `text`). `cu` reads this after `agt_a11y_node_set_text`.
pub(crate) fn last_text_write_via() -> &'static str {
    LAST_TEXT_VIA.with(Cell::get)
}

fn remember_text_via(via: &'static str) {
    LAST_TEXT_VIA.with(|cell| cell.set(via));
}

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

fn shared_connection_slot() -> &'static Mutex<Option<zbus::Connection>> {
    SHARED_CONNECTION.get_or_init(|| Mutex::new(None))
}

fn cached_connection() -> Option<zbus::Connection> {
    shared_connection_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn remember_connection(conn: zbus::Connection) -> zbus::Connection {
    let mut slot = shared_connection_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = slot.as_ref() {
        return existing.clone();
    }
    // Leak one owner so process teardown cannot Drop the zbus connection
    // (that Drop talks to the a11y bus while the runtime is dying).
    let leaked: &'static zbus::Connection = Box::leak(Box::new(conn.clone()));
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

/// Write through AT-SPI `EditableText` when present, otherwise AT-SPI `Text`
/// plus the toolkit's accessibility set-value (Chrome: renderer AX `kSetValue`).
/// Confirmed by `Text.GetText`. A named showing node with no writeable text
/// interface fails typed — never XTest / `GenerateKeyboardEvent`.
pub(crate) fn set_node_text(
    window_handle: Option<isize>,
    node_id: &str,
    text: &str,
) -> Result<(), AccessibilityTreeError> {
    runtime().block_on(async {
        timeout(
            SNAPSHOT_TIMEOUT,
            set_node_text_async(window_handle, node_id, text),
        )
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_text_timeout",
                "AT-SPI text write exceeded its deadline",
            )
        })?
    })
}

/// Deliver a chord through AT-SPI Device/key events (`DeviceEventListener`
/// `NotifyEvent`). A named showing node with no key interface fails typed —
/// never XTest / `input_inject::send_keys`.
pub(crate) fn send_node_keys(
    window_handle: Option<isize>,
    node_id: &str,
    keys: &str,
) -> Result<(), AccessibilityTreeError> {
    runtime().block_on(async {
        timeout(
            SNAPSHOT_TIMEOUT,
            send_node_keys_async(window_handle, node_id, keys),
        )
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_key_timeout",
                "AT-SPI Device/key event exceeded its deadline",
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

    let dbus = DBusProxy::new(&conn).await.ok();
    let mut nodes = Vec::new();
    let mut queue: VecDeque<(BusObject, String, Option<String>, u32)> = VecDeque::new();
    for (index, object) in selected.into_iter().enumerate() {
        queue.push_back((object, format!("/{index}"), None, 0));
    }

    while let Some((object, id, parent_id, depth)) = queue.pop_front() {
        if nodes.len() >= max_nodes {
            break;
        }
        let object =
            match resolve_walk_object(&conn, dbus.as_ref(), identity.as_ref(), object).await {
                Some(object) => object,
                None => continue,
            };
        let Ok(Ok(proxy)) = timeout(NODE_TIMEOUT, open_bus_object(&conn, &object)).await else {
            continue;
        };
        // Read name/role even if Action/Text hang (WebKitGTK GetActions).
        // Never drop the node before enqueueing children — that is how the
        // document embed used to disappear into role=unknown / n=6 fillers.
        let node = read_node(&proxy, id.clone(), parent_id.clone()).await;
        let child_budget = max_nodes.saturating_sub(nodes.len() + queue.len());
        let child_refs = if depth < max_depth && child_budget > 0 {
            timeout(NODE_TIMEOUT, raw_children(&proxy, child_budget))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default()
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
        AccessibilityNodeAction::Click => invoke_structured_click(&proxy).await,
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

async fn set_node_text_async(
    window_handle: Option<isize>,
    node_id: &str,
    text: &str,
) -> Result<(), AccessibilityTreeError> {
    let indices = parse_node_path(node_id)?;
    let conn = connect().await?;
    let identity = window_handle.and_then(window_identity);
    let roots = registry_children(&conn).await?;
    let selected = select_roots(&conn, roots, identity.as_ref()).await?;
    if selected.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!("node path {node_id} has no AT-SPI text interface"),
        ));
    }
    let object = resolve_path(&conn, &selected, &indices).await?;
    let proxy = open_bus_object(&conn, &object).await?;
    invoke_editable_text(&proxy, text, window_handle).await
}

async fn send_node_keys_async(
    window_handle: Option<isize>,
    node_id: &str,
    keys: &str,
) -> Result<(), AccessibilityTreeError> {
    let synth = parse_send_keys(keys)?;
    let indices = parse_node_path(node_id)?;
    let conn = connect().await?;
    let identity = window_handle.and_then(window_identity);
    let roots = registry_children(&conn).await?;
    let selected = select_roots(&conn, roots, identity.as_ref()).await?;
    if selected.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_key_unavailable",
            format!("node path {node_id} has no AT-SPI Device/key interface"),
        ));
    }
    let object = resolve_path(&conn, &selected, &indices).await?;
    let proxy = open_bus_object(&conn, &object).await?;
    invoke_device_keys(&proxy, &synth).await
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

/// One a11y-bus connection per process.
///
/// Do **not** go through `atspi::AccessibilityConnection::new()`. That
/// constructor enables atspi's optional P2P peer table
/// (`GetApplicationBusAddress` plus a unix-socket handshake per registry
/// child). WebKitGTK/Wails sockets hang that handshake, so `cu tree` dies
/// with `a11y_tree_timeout` before it can walk the document embed. Talk to
/// the a11y bus only.
///
/// `send-keys --name` snapshots the tree then delivers Device/key events on
/// the same bus. Opening a fresh connection for each of those steps (and
/// dropping it before the event) tears Chrome's renderer tree down, so the
/// next named command sees `a11y_node_not_found`. Clone the shared
/// connection instead of dropping the a11y bus.
async fn connect() -> Result<zbus::Connection, AccessibilityTreeError> {
    hydrate_session_bus_env();
    if let Some(conn) = cached_connection() {
        return Ok(conn);
    }
    let conn = open_a11y_bus().await?;
    Ok(remember_connection(conn))
}

async fn open_a11y_bus() -> Result<zbus::Connection, AccessibilityTreeError> {
    let session = zbus::Connection::session().await.map_err(map_atspi_err)?;
    let address = a11y_bus_address(&session).await?;
    zbus::connection::Builder::address(address.as_str())
        .map_err(map_atspi_err)?
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn a11y_bus_address(session: &zbus::Connection) -> Result<String, AccessibilityTreeError> {
    if let Ok(value) = std::env::var("AT_SPI_BUS_ADDRESS")
        && !value.is_empty()
    {
        return Ok(value);
    }
    let proxy = zbus::Proxy::new(session, A11Y_BUS_DEST, A11Y_BUS_PATH, A11Y_BUS_IFACE)
        .await
        .map_err(map_atspi_err)?;
    proxy.call("GetAddress", &()).await.map_err(map_atspi_err)
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
fn is_usable_object_ref(object_ref: &atspi::ObjectRefOwned) -> bool {
    !object_ref.is_null()
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

async fn registry_root(
    conn: &zbus::Connection,
) -> Result<AccessibleProxy<'_>, AccessibilityTreeError> {
    AccessibleProxy::builder(conn)
        .destination(REGISTRY_DEST)
        .map_err(map_atspi_err)?
        .path(APPLICATION_ROOT_PATH)
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn registry_children(
    conn: &zbus::Connection,
) -> Result<Vec<BusObject>, AccessibilityTreeError> {
    let root = registry_root(conn).await?;
    let children = raw_children(&root, 256).await?;
    if children.is_empty() {
        let child_count = root.child_count().await.unwrap_or(0);
        if child_count > 0 {
            return Err(AccessibilityTreeError::failed(
                "a11y_registry_read_failed",
                "AT-SPI registry reported children but none could be read",
            ));
        }
    }
    Ok(children)
}

async fn select_roots(
    conn: &zbus::Connection,
    roots: Vec<BusObject>,
    identity: Option<&WindowIdentity>,
) -> Result<Vec<BusObject>, AccessibilityTreeError> {
    let mut selected = Vec::new();
    let dbus = DBusProxy::new(conn).await.ok();
    for object in roots {
        let Some(identity) = identity else {
            selected.push(object);
            continue;
        };
        if root_matches_window(conn, dbus.as_ref(), &object, identity).await {
            selected.push(object);
        }
    }
    if let (Some(identity), Some(dbus)) = (identity, dbus.as_ref()) {
        let extras = extra_roots_for_window(conn, dbus, identity, &selected).await;
        selected.extend(extras);
    }
    Ok(selected)
}

/// When the a11y destination publishes a Unix PID, that PID is
/// authoritative. Two `agenterm-con` processes share toolkit name
/// `agenterm-con`; falling through to name matching would merge both
/// trees and make `--name Command` ambiguous. Name/title fallback stays
/// only for destinations whose PID cannot be read (some embeds).
fn dest_pid_verdict(dest_pid: Option<u32>, owns_pid: bool) -> Option<bool> {
    dest_pid.map(|_| owns_pid)
}

async fn root_matches_window(
    conn: &zbus::Connection,
    dbus: Option<&DBusProxy<'_>>,
    object: &BusObject,
    identity: &WindowIdentity,
) -> bool {
    let pid = dest_pid(dbus, &object.dest).await;
    if let Some(matched) = dest_pid_verdict(pid, pid.is_some_and(|pid| identity.owns_pid(pid))) {
        return matched;
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
    conn: &zbus::Connection,
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
    conn: &zbus::Connection,
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

async fn open_bus_object<'a>(
    conn: &'a zbus::Connection,
    object: &BusObject,
) -> Result<AccessibleProxy<'a>, AccessibilityTreeError> {
    let dbus = DBusProxy::new(conn).await.ok();
    if !dest_is_owned(dbus.as_ref(), &object.dest).await {
        return Err(AccessibilityTreeError::failed(
            "a11y_node_not_found",
            format!("AT-SPI destination {} has no owner", object.dest),
        ));
    }
    let dest = resolve_dest(dbus.as_ref(), &object.dest).await;
    let path = object.path.clone();
    AccessibleProxy::builder(conn)
        .destination(dest)
        .map_err(map_atspi_err)?
        .path(path)
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

/// WebKitGTK embeds the document tree under a well-known dest
/// (`org.webkit.app-*.Sandboxed.WebProcess-*`). GetChildren still names that
/// dest after the web process dies or restarts under a new UUID. Skip the
/// unowned stub (it becomes role=unknown) and, when possible, retarget to a
/// live WebKit dest owned by the same window.
async fn resolve_walk_object(
    conn: &zbus::Connection,
    dbus: Option<&DBusProxy<'_>>,
    identity: Option<&WindowIdentity>,
    object: BusObject,
) -> Option<BusObject> {
    if dest_is_owned(dbus, &object.dest).await {
        return Some(object);
    }
    if !is_webkit_embed_dest(&object.dest) {
        return None;
    }
    recover_webkit_embed(conn, dbus?, identity, &object).await
}

fn is_webkit_embed_dest(dest: &str) -> bool {
    dest.contains("Sandboxed.WebProcess-") || dest.starts_with("org.webkit.")
}

async fn recover_webkit_embed(
    conn: &zbus::Connection,
    dbus: &DBusProxy<'_>,
    identity: Option<&WindowIdentity>,
    original: &BusObject,
) -> Option<BusObject> {
    let names = dbus.list_names().await.ok()?;
    let mut owned_by_window = Vec::new();
    let mut other_webkit = Vec::new();
    for name in names {
        let dest = name.as_str();
        if dest == original.dest || !is_webkit_embed_dest(dest) {
            continue;
        }
        let Ok(bus_name) = BusName::try_from(dest.to_owned()) else {
            continue;
        };
        let Ok(pid) = dbus.get_connection_unix_process_id(bus_name).await else {
            continue;
        };
        let candidate = BusObject {
            dest: dest.to_owned(),
            path: original.path.clone(),
        };
        if identity.is_some_and(|identity| identity.owns_pid(pid)) {
            owned_by_window.push(candidate);
        } else if identity.is_none() {
            other_webkit.push(candidate);
        }
    }
    for candidate in owned_by_window.into_iter().chain(other_webkit) {
        if timeout(NODE_TIMEOUT, open_bus_object(conn, &candidate))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some()
        {
            return Some(candidate);
        }
        // The live dest may not serve the stale embed path. Try the WebKit
        // application root used when the process registered on the host bus.
        let at_root = BusObject {
            dest: candidate.dest.clone(),
            path: APPLICATION_ROOT_PATH.to_owned(),
        };
        if timeout(NODE_TIMEOUT, open_bus_object(conn, &at_root))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some()
        {
            return Some(at_root);
        }
    }
    None
}

async fn dest_is_owned(dbus: Option<&DBusProxy<'_>>, dest: &str) -> bool {
    if dest.is_empty() {
        return false;
    }
    let Some(dbus) = dbus else {
        return true;
    };
    let Ok(bus_name) = BusName::try_from(dest.to_owned()) else {
        return false;
    };
    dbus.get_name_owner(bus_name).await.is_ok()
}

async fn dest_pid(dbus: Option<&DBusProxy<'_>>, dest: &str) -> Option<u32> {
    let dbus = dbus?;
    let bus_name = BusName::try_from(dest.to_string()).ok()?;
    dbus.get_connection_unix_process_id(bus_name).await.ok()
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
    // Stay off Component/Action/`proxies()` during snapshot — WebKitGTK
    // hangs those. GetText on an entry/text/editable node is bounded
    // (ACTION_TIMEOUT) so `cu tree` can show what EditableText wrote.
    let text = if node_looks_like_text_field(&role, &states) {
        timeout(ACTION_TIMEOUT, text_from_text_proxy(proxy))
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    AccessibilityNode {
        id,
        parent_id,
        role,
        name,
        states,
        bounds: AccessibilityBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        actions: Vec::new(),
        text,
    }
}

fn node_looks_like_text_field(role: &str, states: &[String]) -> bool {
    let role = role.to_ascii_lowercase();
    matches!(
        role.as_str(),
        "entry" | "text" | "password text" | "passwordtext" | "edit" | "edit box"
    ) || states.iter().any(|state| state == "editable")
}

async fn role_name(proxy: &AccessibleProxy<'_>) -> String {
    if let Ok(role) = proxy.get_role_name().await
        && !role.trim().is_empty()
    {
        return role;
    }
    match proxy.get_role().await {
        Ok(role) => atspi_role_label(role),
        Err(_) => "unknown".to_string(),
    }
}

/// WebKitGTK leaves `GetRoleName` empty and only answers `GetRole` (e.g. 43 =
/// push button). Map the common ATSPI roles to the same labels GTK publishes
/// so `--name` / `--role` matchers stay toolkit-neutral.
fn atspi_role_label(role: Role) -> String {
    match role {
        Role::Button => "button".to_owned(),
        Role::ToggleButton => "toggle button".to_owned(),
        Role::Entry | Role::PasswordText => "text".to_owned(),
        Role::Text => "text".to_owned(),
        Role::Heading => "heading".to_owned(),
        Role::PageTab => "page tab".to_owned(),
        Role::PageTabList => "page tab list".to_owned(),
        Role::Link => "link".to_owned(),
        Role::CheckBox => "check box".to_owned(),
        Role::RadioButton => "radio button".to_owned(),
        Role::ComboBox => "combo box".to_owned(),
        Role::MenuItem => "menu item".to_owned(),
        Role::Menu => "menu".to_owned(),
        Role::MenuBar => "menu bar".to_owned(),
        Role::ToolBar => "tool bar".to_owned(),
        Role::ScrollBar => "scroll bar".to_owned(),
        Role::Slider => "slider".to_owned(),
        Role::SpinButton => "spin button".to_owned(),
        Role::Image => "image".to_owned(),
        Role::List => "list".to_owned(),
        Role::ListItem => "list item".to_owned(),
        Role::Table => "table".to_owned(),
        Role::TableCell => "table cell".to_owned(),
        Role::DocumentWeb | Role::DocumentFrame => "document web".to_owned(),
        Role::Panel => "panel".to_owned(),
        Role::Filler => "filler".to_owned(),
        Role::Frame => "frame".to_owned(),
        Role::Window => "window".to_owned(),
        Role::Application => "application".to_owned(),
        Role::Section => "section".to_owned(),
        Role::Paragraph => "paragraph".to_owned(),
        Role::Label => "label".to_owned(),
        Role::Static => "static".to_owned(),
        other => {
            let debug = format!("{other:?}");
            if debug.is_empty() {
                "unknown".to_owned()
            } else {
                debug
            }
        }
    }
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
    let performed = action_proxy
        .do_action(index as i32)
        .await
        .map_err(map_atspi_err)?;
    if !performed {
        return Err(AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            format!("AT-SPI DoAction({index}) returned false"),
        ));
    }
    Ok(())
}

/// How a resolved node is clicked. `has_action` is `GetInterfaces`.
/// `None` means the probe timed out — still prefer Action (`DoAction(0)`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClickRoute {
    Action { index: usize },
    Component,
}

fn click_route(has_action: Option<bool>, names: &[String]) -> ClickRoute {
    match has_action {
        Some(false) => ClickRoute::Component,
        _ => ClickRoute::Action {
            index: click_action_index(names).unwrap_or(0),
        },
    }
}

fn extents_center(x: i32, y: i32, width: i32, height: i32) -> Option<(i32, i32)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((x.saturating_add(width / 2), y.saturating_add(height / 2)))
}

async fn node_exposes_action(proxy: &AccessibleProxy<'_>) -> Option<bool> {
    match timeout(ACTION_TIMEOUT, proxy.get_interfaces()).await {
        Ok(Ok(ifaces)) => Some(ifaces.contains(Interface::Action)),
        _ => None,
    }
}

fn is_missing_action_interface(error: &AccessibilityTreeError) -> bool {
    let AccessibilityTreeError::Failed { code, message } = error else {
        return false;
    };
    code == "a11y_action_unavailable"
        || message.contains("UnknownInterface")
        || message.contains("UnknownMethod")
        || message.contains("does not exist")
}

async fn invoke_structured_click(
    proxy: &AccessibleProxy<'_>,
) -> Result<(), AccessibilityTreeError> {
    let has_action = node_exposes_action(proxy).await;
    match click_route(has_action, &[]) {
        ClickRoute::Component => invoke_component_click(proxy).await,
        ClickRoute::Action { .. } => match invoke_action_click(proxy).await {
            Ok(()) => Ok(()),
            Err(action_err)
                if has_action != Some(true) && is_missing_action_interface(&action_err) =>
            {
                invoke_component_click(proxy).await
            }
            Err(action_err) => Err(action_err),
        },
    }
}

async fn invoke_action_click(proxy: &AccessibleProxy<'_>) -> Result<(), AccessibilityTreeError> {
    let action_proxy = action_proxy_for(proxy).await?;
    // WebKitGTK advertises Action but `GetActions` often hangs. Prefer a
    // named click when the list arrives quickly; otherwise invoke the
    // AT-SPI default action at index 0.
    let names = match timeout(ACTION_TIMEOUT, action_names(&action_proxy)).await {
        Ok(Ok(names)) => names,
        _ => Vec::new(),
    };
    let index = click_action_index(&names).unwrap_or(0);
    timeout(NODE_TIMEOUT, do_action_at(&action_proxy, index))
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_action_timeout",
                "AT-SPI DoAction exceeded its deadline",
            )
        })?
}

async fn invoke_component_click(proxy: &AccessibleProxy<'_>) -> Result<(), AccessibilityTreeError> {
    let component = component_proxy_for(proxy).await?;
    let (x, y, width, height) = timeout(NODE_TIMEOUT, component.get_extents(CoordType::Screen))
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_action_timeout",
                "AT-SPI Component GetExtents exceeded its deadline",
            )
        })?
        .map_err(map_atspi_err)?;
    let Some((cx, cy)) = extents_center(x, y, width, height) else {
        return Err(AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            "node Component extents are empty; not falling back to --coords",
        ));
    };
    let dec = DeviceEventControllerProxy::builder(proxy.inner().connection())
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)?;
    timeout(NODE_TIMEOUT, dec.generate_mouse_event(cx, cy, "b1c"))
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_action_timeout",
                "AT-SPI GenerateMouseEvent exceeded its deadline",
            )
        })?
        .map_err(map_atspi_err)
}

async fn action_proxy_for<'a>(
    proxy: &AccessibleProxy<'a>,
) -> Result<ActionProxy<'a>, AccessibilityTreeError> {
    let inner = proxy.inner();
    ActionProxy::builder(inner.connection())
        .destination(inner.destination().to_owned())
        .map_err(map_atspi_err)?
        .path(inner.path().to_owned())
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn component_proxy_for<'a>(
    proxy: &AccessibleProxy<'a>,
) -> Result<ComponentProxy<'a>, AccessibilityTreeError> {
    let inner = proxy.inner();
    ComponentProxy::builder(inner.connection())
        .destination(inner.destination().to_owned())
        .map_err(map_atspi_err)?
        .path(inner.path().to_owned())
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn editable_text_proxy_for<'a>(
    proxy: &AccessibleProxy<'a>,
) -> Result<EditableTextProxy<'a>, AccessibilityTreeError> {
    let inner = proxy.inner();
    EditableTextProxy::builder(inner.connection())
        .destination(inner.destination().to_owned())
        .map_err(map_atspi_err)?
        .path(inner.path().to_owned())
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn device_listener_proxy_for<'a>(
    proxy: &AccessibleProxy<'a>,
) -> Result<DeviceEventListenerProxy<'a>, AccessibilityTreeError> {
    let inner = proxy.inner();
    DeviceEventListenerProxy::builder(inner.connection())
        .destination(inner.destination().to_owned())
        .map_err(map_atspi_err)?
        .path(inner.path().to_owned())
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

async fn text_proxy_for<'a>(
    proxy: &AccessibleProxy<'a>,
) -> Result<TextProxy<'a>, AccessibilityTreeError> {
    let inner = proxy.inner();
    TextProxy::builder(inner.connection())
        .destination(inner.destination().to_owned())
        .map_err(map_atspi_err)?
        .path(inner.path().to_owned())
        .map_err(map_atspi_err)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(map_atspi_err)
}

/// How a resolved node is written. `has_editable` / `has_text` are
/// `GetInterfaces` for `EditableText` / `Text`. `None` means the probe
/// timed out — still try the write (WebKit `GetInterfaces` hangs).
/// Chrome 151 reports `Text` + `editable` but never `EditableText`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextWriteRoute {
    EditableText,
    Text,
    Unavailable,
}

fn text_write_route(
    has_editable: Option<bool>,
    has_text: Option<bool>,
    is_editable_state: bool,
) -> TextWriteRoute {
    match has_editable {
        Some(true) => TextWriteRoute::EditableText,
        None => TextWriteRoute::EditableText,
        Some(false) if is_editable_state && has_text != Some(false) => TextWriteRoute::Text,
        Some(false) => TextWriteRoute::Unavailable,
    }
}

/// `InsertText` length is a character count (libatspi
/// `atspi_editable_text_insert_text`). Cap at i32::MAX; empty is 0.
fn insert_text_char_count(text: &str) -> i32 {
    i32::try_from(text.chars().count()).unwrap_or(i32::MAX)
}

fn is_missing_text_interface(error: &AccessibilityTreeError) -> bool {
    let AccessibilityTreeError::Failed { code, message } = error else {
        return false;
    };
    code == "a11y_text_unavailable"
        || message.contains("UnknownInterface")
        || message.contains("UnknownMethod")
        || message.contains("does not exist")
}

async fn node_interfaces(proxy: &AccessibleProxy<'_>) -> Option<atspi::InterfaceSet> {
    match timeout(ACTION_TIMEOUT, proxy.get_interfaces()).await {
        Ok(Ok(ifaces)) => Some(ifaces),
        _ => None,
    }
}

fn node_has_editable_state(states: &[String]) -> bool {
    states.iter().any(|state| state == "editable")
}

/// How a resolved node receives keys. `has_listener` is `GetInterfaces` for
/// `org.a11y.atspi.DeviceEventListener`. `None` means the probe timed out —
/// still try `NotifyEvent` (same WebKit `GetInterfaces` hang as text write).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyRoute {
    DeviceListener,
    Unavailable,
}

fn key_route(has_listener: Option<bool>) -> KeyRoute {
    match has_listener {
        Some(false) => KeyRoute::Unavailable,
        _ => KeyRoute::DeviceListener,
    }
}

fn is_missing_key_interface(error: &AccessibilityTreeError) -> bool {
    let AccessibilityTreeError::Failed { code, message } = error else {
        return false;
    };
    code == "a11y_key_unavailable"
        || message.contains("UnknownInterface")
        || message.contains("UnknownMethod")
        || message.contains("does not exist")
}

async fn node_exposes_device_listener(proxy: &AccessibleProxy<'_>) -> Option<bool> {
    match timeout(ACTION_TIMEOUT, proxy.get_interfaces()).await {
        Ok(Ok(ifaces)) => Some(ifaces.contains(Interface::DeviceEventListener)),
        _ => None,
    }
}

struct SynthKey {
    keysym: i32,
    event_string: String,
    is_text: bool,
    modifiers: i32,
}

const ATSPI_MOD_SHIFT: i32 = 1 << 0;
const ATSPI_MOD_CONTROL: i32 = 1 << 2;
const ATSPI_MOD_ALT: i32 = 1 << 3;
const ATSPI_MOD_META: i32 = 1 << 4;

fn parse_send_keys(keys: &str) -> Result<SynthKey, AccessibilityTreeError> {
    let parts: Vec<&str> = keys.split('+').map(str::trim).collect();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(AccessibilityTreeError::failed(
            "invalid_input",
            format!("cannot parse shortcut '{keys}'"),
        ));
    }
    let mut modifiers = 0i32;
    for part in &parts[..parts.len() - 1] {
        let mask = modifier_mask(part).ok_or_else(|| {
            AccessibilityTreeError::failed(
                "invalid_input",
                format!("unknown modifier '{part}' in shortcut '{keys}'"),
            )
        })?;
        modifiers |= mask;
    }
    let token = parts[parts.len() - 1];
    let (keysym, event_string, mut is_text) = key_token(token).ok_or_else(|| {
        AccessibilityTreeError::failed(
            "invalid_input",
            format!("unknown key '{token}' in shortcut '{keys}'"),
        )
    })?;
    if modifiers & (ATSPI_MOD_CONTROL | ATSPI_MOD_ALT | ATSPI_MOD_META) != 0 {
        is_text = false;
    }
    Ok(SynthKey {
        keysym,
        event_string,
        is_text,
        modifiers,
    })
}

fn modifier_mask(token: &str) -> Option<i32> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(ATSPI_MOD_CONTROL),
        "shift" => Some(ATSPI_MOD_SHIFT),
        "alt" => Some(ATSPI_MOD_ALT),
        "meta" | "super" | "win" => Some(ATSPI_MOD_META),
        _ => None,
    }
}

fn key_token(token: &str) -> Option<(i32, String, bool)> {
    match token.to_ascii_lowercase().as_str() {
        "backspace" => Some((0xff08, "BackSpace".into(), false)),
        "tab" => Some((0xff09, "Tab".into(), false)),
        "enter" | "return" => Some((0xff0d, "Return".into(), false)),
        "escape" | "esc" => Some((0xff1b, "Escape".into(), false)),
        "space" => Some((0x0020, " ".into(), true)),
        "home" => Some((0xff50, "Home".into(), false)),
        "left" => Some((0xff51, "Left".into(), false)),
        "up" => Some((0xff52, "Up".into(), false)),
        "right" => Some((0xff53, "Right".into(), false)),
        "down" => Some((0xff54, "Down".into(), false)),
        "delete" | "del" => Some((0xffff, "Delete".into(), false)),
        "f1" => Some((0xffbe, "F1".into(), false)),
        "f2" => Some((0xffbf, "F2".into(), false)),
        "f3" => Some((0xffc0, "F3".into(), false)),
        "f4" => Some((0xffc1, "F4".into(), false)),
        "f5" => Some((0xffc2, "F5".into(), false)),
        "f6" => Some((0xffc3, "F6".into(), false)),
        "f7" => Some((0xffc4, "F7".into(), false)),
        "f8" => Some((0xffc5, "F8".into(), false)),
        "f9" => Some((0xffc6, "F9".into(), false)),
        "f10" => Some((0xffc7, "F10".into(), false)),
        "f11" => Some((0xffc8, "F11".into(), false)),
        "f12" => Some((0xffc9, "F12".into(), false)),
        other => {
            let mut chars = other.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            let keysym = i32::try_from(u32::from(ch)).ok()?;
            Some((keysym, ch.to_string(), !ch.is_control()))
        }
    }
}

/// Native AT-SPI Device/key: `DeviceEventListener.NotifyEvent` press+release.
/// Never falls through to XTest / `DeviceEventController.GenerateKeyboardEvent`.
async fn invoke_device_keys(
    proxy: &AccessibleProxy<'_>,
    synth: &SynthKey,
) -> Result<(), AccessibilityTreeError> {
    let has_listener = node_exposes_device_listener(proxy).await;
    match key_route(has_listener) {
        KeyRoute::Unavailable => Err(AccessibilityTreeError::failed(
            "a11y_key_unavailable",
            "node does not expose the AT-SPI DeviceEventListener interface",
        )),
        KeyRoute::DeviceListener => match notify_device_keys(proxy, synth).await {
            Ok(()) => Ok(()),
            Err(send_err) if has_listener != Some(true) && is_missing_key_interface(&send_err) => {
                Err(AccessibilityTreeError::failed(
                    "a11y_key_unavailable",
                    "node does not expose the AT-SPI DeviceEventListener interface",
                ))
            }
            Err(send_err) => Err(send_err),
        },
    }
}

async fn notify_device_keys(
    proxy: &AccessibleProxy<'_>,
    synth: &SynthKey,
) -> Result<(), AccessibilityTreeError> {
    let listener = device_listener_proxy_for(proxy).await?;
    let pressed = DeviceEvent {
        event_type: EventType::KeyPressed,
        id: synth.keysym,
        hw_code: 0,
        modifiers: synth.modifiers,
        timestamp: 0,
        event_string: synth.event_string.as_str(),
        is_text: synth.is_text,
    };
    let accepted = timeout(NODE_TIMEOUT, listener.notify_event(&pressed))
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_key_timeout",
                "AT-SPI DeviceEventListener NotifyEvent exceeded its deadline",
            )
        })?
        .map_err(map_atspi_err)?;
    if !accepted {
        return Err(AccessibilityTreeError::failed(
            "a11y_key_unavailable",
            "AT-SPI DeviceEventListener NotifyEvent returned false",
        ));
    }
    let released = DeviceEvent {
        event_type: EventType::KeyReleased,
        ..pressed
    };
    match timeout(NODE_TIMEOUT, listener.notify_event(&released)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            let mapped = map_atspi_err(error);
            if !is_missing_key_interface(&mapped) {
                return Err(mapped);
            }
        }
        Err(_) => {}
    }
    Ok(())
}

async fn text_from_text_proxy(proxy: &AccessibleProxy<'_>) -> Option<String> {
    let text = text_proxy_for(proxy).await.ok()?;
    let count = text.character_count().await.ok()?.clamp(0, 4096);
    if count == 0 {
        return None;
    }
    text.get_text(0, count)
        .await
        .ok()
        .filter(|value| !value.is_empty())
}

async fn text_character_count(proxy: &AccessibleProxy<'_>) -> Option<i32> {
    let text = timeout(ACTION_TIMEOUT, text_proxy_for(proxy))
        .await
        .ok()?
        .ok()?;
    timeout(ACTION_TIMEOUT, text.character_count())
        .await
        .ok()?
        .ok()
        .map(|count| count.max(0))
}

/// Native AT-SPI write: `EditableText.SetTextContents`, then
/// `EditableText.InsertText`. Matches libatspi
/// `atspi_editable_text_set_text_contents` / `atspi_editable_text_insert_text`.
/// Chrome named fields expose `Text` but not `EditableText`; those take the
/// `Text` route (renderer AX set-value, confirmed by `GetText`). Never
/// falls through to XTest / DeviceEventController keyboard.
async fn invoke_editable_text(
    proxy: &AccessibleProxy<'_>,
    text: &str,
    window_handle: Option<isize>,
) -> Result<(), AccessibilityTreeError> {
    let ifaces = node_interfaces(proxy).await;
    let has_editable = ifaces.map(|set| set.contains(Interface::EditableText));
    let has_text = ifaces.map(|set| set.contains(Interface::Text));
    let states = states_from_proxy(proxy).await;
    let is_editable_state = node_has_editable_state(&states);
    match text_write_route(has_editable, has_text, is_editable_state) {
        TextWriteRoute::Unavailable => Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            "node does not expose the AT-SPI EditableText interface",
        )),
        TextWriteRoute::EditableText => match write_editable_text(proxy, text).await {
            Ok(()) => {
                verify_text_contents(proxy, text).await?;
                remember_text_via("editable-text");
                Ok(())
            }
            Err(write_err)
                if has_editable != Some(true) && is_missing_text_interface(&write_err) =>
            {
                if is_editable_state && has_text != Some(false) {
                    write_via_atspi_text(proxy, text, window_handle).await
                } else {
                    Err(AccessibilityTreeError::failed(
                        "a11y_text_unavailable",
                        "node does not expose the AT-SPI EditableText interface",
                    ))
                }
            }
            Err(write_err) => Err(write_err),
        },
        TextWriteRoute::Text => write_via_atspi_text(proxy, text, window_handle).await,
    }
}

async fn write_via_atspi_text(
    proxy: &AccessibleProxy<'_>,
    text: &str,
    window_handle: Option<isize>,
) -> Result<(), AccessibilityTreeError> {
    if let Ok(component) = component_proxy_for(proxy).await {
        let _ = timeout(NODE_TIMEOUT, component.grab_focus()).await;
    }
    if let Ok(text_proxy) = text_proxy_for(proxy).await {
        let _ = timeout(ACTION_TIMEOUT, text_proxy.set_caret_offset(0)).await;
        if let Some(count) = text_character_count(proxy).await {
            let _ = timeout(ACTION_TIMEOUT, text_proxy.set_selection(0, 0, count)).await;
        }
    }
    let name = timeout(ACTION_TIMEOUT, proxy.name())
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    if name.trim().is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            "editable AT-SPI Text node has no accessible name for write",
        ));
    }
    let identity = window_handle.and_then(window_identity);
    let pids = identity
        .as_ref()
        .map(|identity| {
            let mut pids = Vec::new();
            if let Some(pid) = identity.pid {
                pids.push(pid);
            }
            pids.extend(identity.descendant_pids.iter().copied());
            pids
        })
        .unwrap_or_default();
    chrome_ax_set_value::set_named_field_value(pids, &name, text)?;
    verify_text_contents(proxy, text).await?;
    remember_text_via("text");
    Ok(())
}

async fn read_text_contents(proxy: &AccessibleProxy<'_>) -> Option<String> {
    let text = timeout(ACTION_TIMEOUT, text_proxy_for(proxy))
        .await
        .ok()?
        .ok()?;
    let count = timeout(ACTION_TIMEOUT, text.character_count())
        .await
        .ok()?
        .ok()?
        .clamp(0, 4096);
    timeout(ACTION_TIMEOUT, text.get_text(0, count))
        .await
        .ok()
        .and_then(Result::ok)
}

async fn verify_text_contents(
    proxy: &AccessibleProxy<'_>,
    expected: &str,
) -> Result<(), AccessibilityTreeError> {
    for _ in 0..10 {
        if read_text_contents(proxy).await.as_deref() == Some(expected) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    let got = read_text_contents(proxy).await;
    Err(AccessibilityTreeError::failed(
        "a11y_text_unavailable",
        format!(
            "AT-SPI Text contents did not become {expected:?} after write (got {got:?}); \
             refusing to report success"
        ),
    ))
}

async fn write_editable_text(
    proxy: &AccessibleProxy<'_>,
    text: &str,
) -> Result<(), AccessibilityTreeError> {
    let editable = editable_text_proxy_for(proxy).await?;
    match timeout(NODE_TIMEOUT, editable.set_text_contents(text)).await {
        Ok(Ok(true)) => return Ok(()),
        Ok(Ok(false)) => {}
        Ok(Err(error)) => {
            let mapped = map_atspi_err(error);
            if is_missing_text_interface(&mapped) {
                return Err(mapped);
            }
        }
        Err(_) => {}
    }
    let position = text_character_count(proxy).await.unwrap_or(0);
    let length = insert_text_char_count(text);
    let inserted = timeout(NODE_TIMEOUT, editable.insert_text(position, text, length))
        .await
        .map_err(|_| {
            AccessibilityTreeError::failed(
                "a11y_text_timeout",
                "AT-SPI InsertText exceeded its deadline",
            )
        })?
        .map_err(map_atspi_err)?;
    if !inserted {
        return Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!("AT-SPI InsertText({position}) returned false"),
        ));
    }
    Ok(())
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
        assert!(!is_usable_object_ref(&atspi::ObjectRefOwned::default()));
        assert!(is_usable_object_ref(
            &atspi::ObjectRefOwned::from_static_str_unchecked(
                ":1.1",
                "/org/a11y/atspi/accessible/1"
            )
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
    fn click_route_uses_component_when_action_interface_is_absent() {
        assert_eq!(click_route(Some(false), &[]), ClickRoute::Component);
        assert_eq!(
            click_route(Some(false), &["click".into()]),
            ClickRoute::Component
        );
    }

    #[test]
    fn click_route_prefers_action_when_present_or_unknown() {
        assert_eq!(
            click_route(Some(true), &[String::new()]),
            ClickRoute::Action { index: 0 }
        );
        assert_eq!(
            click_route(None, &["press".into()]),
            ClickRoute::Action { index: 0 }
        );
        assert_eq!(
            click_route(Some(true), &["focus".into(), "Click".into()]),
            ClickRoute::Action { index: 1 }
        );
    }

    #[test]
    fn extents_center_rejects_empty_component() {
        assert_eq!(extents_center(10, 20, 0, 10), None);
        assert_eq!(extents_center(10, 20, 30, 0), None);
        assert_eq!(extents_center(10, 20, 30, 10), Some((25, 25)));
    }

    #[test]
    fn text_write_route_unavailable_when_editable_text_absent_and_not_editable() {
        assert_eq!(
            text_write_route(Some(false), Some(true), false),
            TextWriteRoute::Unavailable
        );
        assert_eq!(
            text_write_route(Some(false), Some(false), true),
            TextWriteRoute::Unavailable
        );
    }

    #[test]
    fn text_write_route_uses_text_when_editable_without_editable_text() {
        assert_eq!(
            text_write_route(Some(false), Some(true), true),
            TextWriteRoute::Text
        );
        assert_eq!(
            text_write_route(Some(false), None, true),
            TextWriteRoute::Text
        );
    }

    #[test]
    fn text_write_route_tries_write_when_present_or_unknown() {
        assert_eq!(
            text_write_route(Some(true), Some(true), true),
            TextWriteRoute::EditableText
        );
        assert_eq!(
            text_write_route(None, None, true),
            TextWriteRoute::EditableText
        );
    }

    #[test]
    fn insert_text_length_is_unicode_scalar_count() {
        assert_eq!(insert_text_char_count(""), 0);
        assert_eq!(insert_text_char_count("hi"), 2);
        assert_eq!(insert_text_char_count("héllo"), 5);
    }

    #[test]
    fn key_route_unavailable_when_device_listener_absent() {
        assert_eq!(key_route(Some(false)), KeyRoute::Unavailable);
        assert_eq!(key_route(Some(true)), KeyRoute::DeviceListener);
        assert_eq!(key_route(None), KeyRoute::DeviceListener);
    }

    #[test]
    fn send_keys_tokens_map_to_device_events() {
        let enter = parse_send_keys("enter").expect("enter");
        assert_eq!(enter.keysym, 0xff0d);
        assert_eq!(enter.event_string, "Return");
        assert!(!enter.is_text);
        let letter = parse_send_keys("k").expect("k");
        assert_eq!(letter.keysym, i32::from(b'k'));
        assert!(letter.is_text);
        let chord = parse_send_keys("ctrl+a").expect("ctrl+a");
        assert_eq!(chord.modifiers, ATSPI_MOD_CONTROL);
        assert!(!chord.is_text);
        assert!(parse_send_keys("ctrl+").is_err());
        assert!(parse_send_keys("hello").is_err());
    }

    #[test]
    fn missing_key_interface_is_typed() {
        assert!(is_missing_key_interface(&AccessibilityTreeError::failed(
            "a11y_key_unavailable",
            "node does not expose the AT-SPI DeviceEventListener interface",
        )));
        assert!(is_missing_key_interface(&AccessibilityTreeError::failed(
            "a11y_backend_failed",
            "org.freedesktop.DBus.Error.UnknownInterface: Interface does not exist",
        )));
        assert!(!is_missing_key_interface(&AccessibilityTreeError::failed(
            "a11y_key_timeout",
            "AT-SPI DeviceEventListener NotifyEvent exceeded its deadline"
        )));
    }

    #[test]
    fn missing_text_interface_is_typed() {
        assert!(is_missing_text_interface(&AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            "node does not expose the AT-SPI EditableText interface",
        )));
        assert!(is_missing_text_interface(&AccessibilityTreeError::failed(
            "a11y_backend_failed",
            "org.freedesktop.DBus.Error.UnknownInterface: Interface does not exist",
        )));
        assert!(!is_missing_text_interface(&AccessibilityTreeError::failed(
            "a11y_text_timeout",
            "AT-SPI InsertText exceeded its deadline"
        )));
    }

    #[test]
    fn text_like_roles_are_read_during_snapshot() {
        assert!(node_looks_like_text_field("entry", &[]));
        assert!(node_looks_like_text_field("text", &[]));
        assert!(node_looks_like_text_field(
            "button",
            &["editable".to_owned()]
        ));
        assert!(!node_looks_like_text_field(
            "button",
            &["showing".to_owned()]
        ));
        assert!(!node_looks_like_text_field("document web", &[]));
    }

    #[test]
    fn missing_action_interface_is_typed() {
        assert!(is_missing_action_interface(
            &AccessibilityTreeError::failed(
                "a11y_action_unavailable",
                "node does not expose the AT-SPI Action interface",
            )
        ));
        assert!(is_missing_action_interface(
            &AccessibilityTreeError::failed(
                "a11y_backend_failed",
                "org.freedesktop.DBus.Error.UnknownMethod: Method does not exist",
            )
        ));
        assert!(!is_missing_action_interface(
            &AccessibilityTreeError::failed(
                "a11y_action_timeout",
                "AT-SPI DoAction exceeded its deadline"
            )
        ));
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
    fn webkit_embed_dests_are_recognized() {
        assert!(is_webkit_embed_dest(
            "org.webkit.app-deadbeef.Sandboxed.WebProcess-uuid"
        ));
        assert!(is_webkit_embed_dest(
            "org.webkitgtk.MiniBrowser.Sandboxed.WebProcess-9448d95f-7bc7-471a-b248-4ff12dd835dd"
        ));
        assert!(is_webkit_embed_dest("org.webkit.Something"));
        assert!(!is_webkit_embed_dest(":1.47"));
        assert!(!is_webkit_embed_dest("org.a11y.atspi.Registry"));
    }

    #[test]
    fn webkit_numeric_roles_map_to_gtk_labels() {
        assert_eq!(atspi_role_label(Role::Button), "button");
        assert_eq!(atspi_role_label(Role::Entry), "text");
        assert_eq!(atspi_role_label(Role::PageTab), "page tab");
        assert_eq!(atspi_role_label(Role::Heading), "heading");
        assert_eq!(atspi_role_label(Role::Filler), "filler");
    }

    #[test]
    fn click_falls_back_to_default_index_when_action_names_time_out() {
        // WebKit GetActions hang: empty name list still has a default action.
        assert_eq!(click_action_index(&[]).unwrap_or(0), 0);
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
    fn known_dest_pid_is_authoritative() {
        assert_eq!(dest_pid_verdict(Some(10), true), Some(true));
        assert_eq!(dest_pid_verdict(Some(11), false), Some(false));
        assert_eq!(dest_pid_verdict(None, false), None);
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
