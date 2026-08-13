//! Linux AT-SPI2 accessibility tree and node actuation.

use std::collections::VecDeque;
use std::sync::OnceLock;

use atspi::connection::AccessibilityConnection;
use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{CoordType, ObjectRefOwned, StateSet};
use tokio::time::{Duration, timeout};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNode, AccessibilityNodeAction, AccessibilityTree,
    AccessibilityTreeError,
};

const MAX_NODES: usize = 1_000;
const MAX_DEPTH: u32 = 32;
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime for AT-SPI")
    })
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
    let target_pid = window_handle.and_then(window_pid);
    let roots = registry_children(&conn).await?;
    let selected = select_roots(&conn, roots, target_pid).await?;
    if selected.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_tree_empty",
            "no AT-SPI application roots matched the requested window",
        ));
    }

    let mut nodes = Vec::new();
    let mut queue: VecDeque<(ObjectRefOwned, String, Option<String>, u32)> = VecDeque::new();
    for (index, object_ref) in selected.into_iter().enumerate() {
        queue.push_back((object_ref, format!("/{index}"), None, 0));
    }

    while let Some((object_ref, id, parent_id, depth)) = queue.pop_front() {
        if nodes.len() >= max_nodes {
            break;
        }
        let proxy = open_accessible(&conn, &object_ref).await?;
        let node = read_node(&proxy, id.clone(), parent_id.clone()).await;
        let child_budget = max_nodes.saturating_sub(nodes.len() + queue.len());
        let child_refs = if depth < max_depth && child_budget > 0 {
            children_up_to(&proxy, child_budget).await?
        } else {
            Vec::new()
        };
        nodes.push(node);
        for (child_index, child_ref) in child_refs.into_iter().enumerate() {
            let child_id = format!("{id}/{child_index}");
            queue.push_back((child_ref, child_id, Some(id.clone()), depth + 1));
        }
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
    let target_pid = window_handle.and_then(window_pid);
    let roots = registry_children(&conn).await?;
    let selected = select_roots(&conn, roots, target_pid).await?;
    let object_ref = resolve_path(&conn, &selected, &indices).await?;
    let proxy = open_accessible(&conn, &object_ref).await?;
    match action {
        AccessibilityNodeAction::Click => invoke_click_action(&proxy).await,
        AccessibilityNodeAction::Focus => {
            if invoke_named_action(&proxy, &["focus"]).await.is_ok() {
                return Ok(());
            }
            let proxies = proxy.proxies().await.map_err(map_atspi_err)?;
            let component = proxies
                .component()
                .await
                .map_err(|error| map_atspi_err(error))?;
            component
                .grab_focus()
                .await
                .map_err(map_atspi_err)
                .map(|_| ())
        }
    }
}

async fn connect() -> Result<AccessibilityConnection, AccessibilityTreeError> {
    hydrate_session_bus_env();
    AccessibilityConnection::new()
        .await
        .map_err(|error| AccessibilityTreeError::failed("a11y_connect_failed", error.to_string()))
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
) -> Result<ObjectRefOwned, AccessibilityTreeError> {
    let child_count = proxy.child_count().await.map_err(map_atspi_err)?;
    let count = usize::try_from(child_count).unwrap_or(0);
    let mut seen = 0usize;
    for index in 0..count {
        let Some(child) = child_at_index_skip_null(proxy, index as i32).await? else {
            continue;
        };
        if seen == logical_index {
            return Ok(child);
        }
        seen += 1;
    }
    Err(AccessibilityTreeError::failed(
        "a11y_node_not_found",
        format!("child index {logical_index} is unavailable"),
    ))
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
    target_pid: Option<u32>,
) -> Result<Vec<ObjectRefOwned>, AccessibilityTreeError> {
    let Some(target_pid) = target_pid else {
        return Ok(roots);
    };
    let dbus = DBusProxy::new(conn.connection())
        .await
        .map_err(map_atspi_err)?;
    let mut matches = Vec::new();
    for object_ref in roots {
        if object_ref_pid(Some(&dbus), &object_ref).await == Some(target_pid) {
            matches.push(object_ref);
        }
    }
    Ok(matches)
}

async fn resolve_path(
    conn: &AccessibilityConnection,
    roots: &[ObjectRefOwned],
    indices: &[usize],
) -> Result<ObjectRefOwned, AccessibilityTreeError> {
    let Some((&root_index, rest)) = indices.split_first() else {
        return Err(AccessibilityTreeError::failed(
            "a11y_node_not_found",
            "node path is empty",
        ));
    };
    let object_ref = roots.get(root_index).cloned().ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_node_not_found",
            format!("application root index {root_index} is out of range"),
        )
    })?;
    let mut current = object_ref;
    for &child_index in rest {
        let proxy = open_accessible(conn, &current).await?;
        current = child_at_logical_index(&proxy, child_index).await?;
    }
    Ok(current)
}

async fn open_accessible<'a>(
    conn: &'a AccessibilityConnection,
    object_ref: &'a ObjectRefOwned,
) -> Result<AccessibleProxy<'a>, AccessibilityTreeError> {
    object_ref
        .as_accessible_proxy(conn.connection())
        .await
        .map_err(map_atspi_err)
}

async fn children_up_to(
    proxy: &AccessibleProxy<'_>,
    limit: usize,
) -> Result<Vec<ObjectRefOwned>, AccessibilityTreeError> {
    let child_count = proxy.child_count().await.map_err(map_atspi_err)?;
    let count = usize::try_from(child_count).unwrap_or(0);
    let mut children = Vec::new();
    for index in 0..count {
        if children.len() >= limit {
            break;
        }
        if let Some(child) = child_at_index_skip_null(proxy, index as i32).await? {
            children.push(child);
        }
    }
    Ok(children)
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
    if let Ok(role) = proxy.get_role_name().await {
        if !role.trim().is_empty() {
            return role;
        }
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
    let count = text.character_count().await.ok()?.max(0).min(4096);
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

fn window_pid(window_handle: isize) -> Option<u32> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    if window_handle == 0 {
        return None;
    }
    let (connection, _screen) = x11rb::connect(None).ok()?;
    let atom = connection
        .intern_atom(false, b"_NET_WM_PID")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let reply = connection
        .get_property(false, window_handle as u32, atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
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
}
