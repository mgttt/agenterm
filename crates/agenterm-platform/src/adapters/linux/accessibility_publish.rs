//! Linux AT-SPI2 application publisher for custom-raster toolkits.
//!
//! winit/softbuffer never loads GTK's atk-bridge, so a process that only
//! paints pixels is invisible on the a11y bus. This adapter registers the
//! process as an AT-SPI application and serves Accessible/Component/Action
//! children. `cu` then walks those children the same way it walks GTK/Chrome.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use atspi::connection::AccessibilityConnection;
use atspi::proxy::socket::SocketProxy;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::accessibility_publish::{
    AccessibilityPublishError, AccessibilityPublisher, PublishedAction, PublishedActionHandler,
    PublishedNode, PublishedRole, PublishedTree,
};
use crate::contract::accessibility_tree::AccessibilityBounds;

const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const NULL_PATH: &str = "/org/a11y/atspi/null";
const OBJECT_PREFIX: &str = "/org/a11y/atspi/accessible/";
const START_TIMEOUT: Duration = Duration::from_secs(5);

const STATE_EDITABLE: u64 = 1 << 7;
const STATE_ENABLED: u64 = 1 << 8;
const STATE_FOCUSABLE: u64 = 1 << 11;
const STATE_FOCUSED: u64 = 1 << 12;
const STATE_SENSITIVE: u64 = 1 << 24;
const STATE_SHOWING: u64 = 1 << 25;
const STATE_SINGLE_LINE: u64 = 1 << 26;
const STATE_VISIBLE: u64 = 1 << 30;

static RUNTIME: OnceLock<&'static tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        Box::leak(Box::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for AT-SPI publish"),
        ))
    })
}

struct Store {
    tree: PublishedTree,
    window_handle: Option<i64>,
    unique_name: String,
    app_id: i32,
    focused: Option<u32>,
    handler: Option<PublishedActionHandler>,
}

impl Store {
    fn node(&self, id: u32) -> Option<&PublishedNode> {
        self.tree.node(id)
    }

    fn children(&self, id: u32) -> Vec<u32> {
        self.tree.children_of(id)
    }
}

#[derive(Clone)]
struct Ctx {
    store: Arc<Mutex<Store>>,
    id: u32,
}

struct AccessibleNode(Ctx);
struct ComponentNode(Ctx);
struct ActionNode(Ctx);
struct ApplicationRoot(Ctx);

fn lock_store(store: &Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn object_path(id: u32) -> String {
    if id == 0 {
        ROOT_PATH.to_owned()
    } else {
        format!("{OBJECT_PREFIX}{id}")
    }
}

fn owned_path(path: &str) -> OwnedObjectPath {
    ObjectPath::try_from(path)
        .map(OwnedObjectPath::from)
        .unwrap_or_else(|_| ObjectPath::from_static_str_unchecked(NULL_PATH).into())
}

fn null_ref() -> (String, OwnedObjectPath) {
    (String::new(), owned_path(NULL_PATH))
}

fn object_ref(unique: &str, id: u32) -> (String, OwnedObjectPath) {
    (unique.to_owned(), owned_path(&object_path(id)))
}

fn state_bits(node: &PublishedNode, focused: Option<u32>) -> u64 {
    let mut bits = STATE_ENABLED | STATE_SENSITIVE | STATE_SHOWING | STATE_VISIBLE;
    if node.focusable {
        bits |= STATE_FOCUSABLE;
    }
    if node.focused || focused == Some(node.id) {
        bits |= STATE_FOCUSED;
    }
    if node.editable {
        bits |= STATE_EDITABLE | STATE_SINGLE_LINE;
    }
    bits
}

fn encode_states(bits: u64) -> Vec<u32> {
    #[allow(clippy::cast_possible_truncation)]
    let low = bits as u32;
    let high = (bits >> 32) as u32;
    vec![low, high]
}

fn screen_origin(window_handle: Option<i64>) -> (i32, i32) {
    let Some(handle) = window_handle else {
        return (0, 0);
    };
    window_screen_origin(handle).unwrap_or((0, 0))
}

fn window_screen_origin(window_handle: i64) -> Option<(i32, i32)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt as _;

    let window = u32::try_from(window_handle).ok()?;
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let translated = connection
        .translate_coordinates(window, root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    Some((i32::from(translated.dst_x), i32::from(translated.dst_y)))
}

fn extents_for(
    node: &PublishedNode,
    window_handle: Option<i64>,
    coord_type: u32,
) -> (i32, i32, i32, i32) {
    let (origin_x, origin_y) = if coord_type == 0 {
        screen_origin(window_handle)
    } else {
        (0, 0)
    };
    (
        origin_x.saturating_add(node.bounds.x),
        origin_y.saturating_add(node.bounds.y),
        node.bounds.width,
        node.bounds.height,
    )
}

fn interfaces_for(node: &PublishedNode) -> Vec<String> {
    let mut names = vec![
        "org.a11y.atspi.Accessible".to_owned(),
        "org.a11y.atspi.Component".to_owned(),
    ];
    if node.id == 0 {
        names.push("org.a11y.atspi.Application".to_owned());
        names.push("org.a11y.atspi.Socket".to_owned());
    }
    if node.clickable || node.focusable {
        names.push("org.a11y.atspi.Action".to_owned());
    }
    names
}

fn actions_for(node: &PublishedNode) -> Vec<(String, String, String)> {
    let mut actions = Vec::new();
    if node.clickable {
        actions.push(("click".into(), "Activate the control".into(), String::new()));
    }
    if node.focusable {
        actions.push((
            "focus".into(),
            "Give the control keyboard focus".into(),
            String::new(),
        ));
    }
    actions
}

#[interface(name = "org.a11y.atspi.Accessible")]
impl AccessibleNode {
    fn get_role(&self) -> u32 {
        let store = lock_store(&self.0.store);
        store
            .node(self.0.id)
            .map(|node| node.role.atspi_role())
            .unwrap_or(67)
    }

    fn get_role_name(&self) -> String {
        let store = lock_store(&self.0.store);
        store
            .node(self.0.id)
            .map(|node| node.role.as_str().to_owned())
            .unwrap_or_else(|| "unknown".into())
    }

    fn get_localized_role_name(&self) -> String {
        self.get_role_name()
    }

    fn get_state(&self) -> Vec<u32> {
        let store = lock_store(&self.0.store);
        store
            .node(self.0.id)
            .map(|node| encode_states(state_bits(node, store.focused)))
            .unwrap_or_else(|| encode_states(0))
    }

    fn get_child_at_index(&self, index: i32) -> (String, OwnedObjectPath) {
        let store = lock_store(&self.0.store);
        let Ok(index) = usize::try_from(index) else {
            return null_ref();
        };
        match store.children(self.0.id).get(index).copied() {
            Some(child) => object_ref(&store.unique_name, child),
            None => null_ref(),
        }
    }

    fn get_children(&self) -> Vec<(String, OwnedObjectPath)> {
        let store = lock_store(&self.0.store);
        store
            .children(self.0.id)
            .into_iter()
            .map(|id| object_ref(&store.unique_name, id))
            .collect()
    }

    fn get_index_in_parent(&self) -> i32 {
        let store = lock_store(&self.0.store);
        let Some(node) = store.node(self.0.id) else {
            return -1;
        };
        let Some(parent) = node.parent else {
            return 0;
        };
        store
            .children(parent)
            .iter()
            .position(|id| *id == self.0.id)
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1)
    }

    fn get_interfaces(&self) -> Vec<String> {
        let store = lock_store(&self.0.store);
        store
            .node(self.0.id)
            .map(interfaces_for)
            .unwrap_or_else(|| vec!["org.a11y.atspi.Accessible".into()])
    }

    fn get_application(&self) -> (String, OwnedObjectPath) {
        let store = lock_store(&self.0.store);
        object_ref(&store.unique_name, 0)
    }

    fn get_attributes(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn get_relation_set(&self) -> Vec<(u32, Vec<(String, OwnedObjectPath)>)> {
        Vec::new()
    }

    #[zbus(property)]
    fn name(&self) -> String {
        let store = lock_store(&self.0.store);
        store
            .node(self.0.id)
            .map(|node| node.name.clone())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn description(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn locale(&self) -> String {
        "C".into()
    }

    #[zbus(property)]
    fn child_count(&self) -> i32 {
        let store = lock_store(&self.0.store);
        i32::try_from(store.children(self.0.id).len()).unwrap_or(0)
    }

    #[zbus(property)]
    fn accessible_id(&self) -> String {
        self.0.id.to_string()
    }

    #[zbus(property)]
    fn help_text(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn parent(&self) -> (String, OwnedObjectPath) {
        let store = lock_store(&self.0.store);
        match store.node(self.0.id).and_then(|node| node.parent) {
            Some(parent) => object_ref(&store.unique_name, parent),
            None => null_ref(),
        }
    }
}

#[interface(name = "org.a11y.atspi.Component")]
impl ComponentNode {
    fn contains(&self, x: i32, y: i32, coord_type: u32) -> bool {
        let store = lock_store(&self.0.store);
        let Some(node) = store.node(self.0.id) else {
            return false;
        };
        let (left, top, width, height) = extents_for(node, store.window_handle, coord_type);
        x >= left && y >= top && x < left.saturating_add(width) && y < top.saturating_add(height)
    }

    fn get_accessible_at_point(
        &self,
        _x: i32,
        _y: i32,
        _coord_type: u32,
    ) -> (String, OwnedObjectPath) {
        null_ref()
    }

    fn get_alpha(&self) -> f64 {
        1.0
    }

    fn get_extents(&self, coord_type: u32) -> (i32, i32, i32, i32) {
        let store = lock_store(&self.0.store);
        store
            .node(self.0.id)
            .map(|node| extents_for(node, store.window_handle, coord_type))
            .unwrap_or((0, 0, 0, 0))
    }

    fn get_layer(&self) -> u32 {
        if self.0.id == 1 { 7 } else { 3 }
    }

    #[zbus(name = "GetMDIZOrder")]
    fn get_mdiz_order(&self) -> i16 {
        0
    }

    fn get_position(&self, coord_type: u32) -> (i32, i32) {
        let (x, y, _, _) = self.get_extents(coord_type);
        (x, y)
    }

    fn get_size(&self) -> (i32, i32) {
        let (_, _, width, height) = self.get_extents(0);
        (width, height)
    }

    fn grab_focus(&self) -> bool {
        let handler = {
            let mut store = lock_store(&self.0.store);
            let Some(node) = store.node(self.0.id) else {
                return false;
            };
            if !node.focusable {
                return false;
            }
            store.focused = Some(self.0.id);
            store.handler.clone()
        };
        if let Some(handler) = handler {
            handler(self.0.id, PublishedAction::Focus);
        }
        true
    }

    fn scroll_to(&self, _type: u32) -> bool {
        false
    }

    fn scroll_to_point(&self, _coord_type: u32, _x: i32, _y: i32) -> bool {
        false
    }

    fn set_extents(&self, _x: i32, _y: i32, _width: i32, _height: i32, _coord_type: u32) -> bool {
        false
    }

    fn set_position(&self, _x: i32, _y: i32, _coord_type: u32) -> bool {
        false
    }

    fn set_size(&self, _width: i32, _height: i32) -> bool {
        false
    }
}

#[interface(name = "org.a11y.atspi.Action")]
impl ActionNode {
    fn do_action(&self, index: i32) -> bool {
        let (action, handler) = {
            let store = lock_store(&self.0.store);
            let Some(node) = store.node(self.0.id) else {
                return false;
            };
            let actions = actions_for(node);
            let Ok(index) = usize::try_from(index) else {
                return false;
            };
            let Some((name, _, _)) = actions.get(index) else {
                return false;
            };
            let action = if name == "focus" {
                PublishedAction::Focus
            } else {
                PublishedAction::Click
            };
            (action, store.handler.clone())
        };
        if action == PublishedAction::Focus {
            lock_store(&self.0.store).focused = Some(self.0.id);
        }
        if let Some(handler) = handler {
            handler(self.0.id, action);
        }
        true
    }

    fn get_actions(&self) -> Vec<(String, String, String)> {
        let store = lock_store(&self.0.store);
        store.node(self.0.id).map(actions_for).unwrap_or_default()
    }

    fn get_description(&self, index: i32) -> String {
        self.get_actions()
            .get(usize::try_from(index).unwrap_or(usize::MAX))
            .map(|action| action.1.clone())
            .unwrap_or_default()
    }

    fn get_name(&self, index: i32) -> String {
        self.get_actions()
            .get(usize::try_from(index).unwrap_or(usize::MAX))
            .map(|action| action.0.clone())
            .unwrap_or_default()
    }

    fn get_localized_name(&self, index: i32) -> String {
        self.get_name(index)
    }

    fn get_key_binding(&self, _index: i32) -> String {
        String::new()
    }

    #[zbus(property, name = "NActions")]
    fn n_actions(&self) -> i32 {
        i32::try_from(self.get_actions().len()).unwrap_or(0)
    }
}

#[interface(name = "org.a11y.atspi.Application")]
impl ApplicationRoot {
    fn get_locale(&self, _lctype: u32) -> String {
        "C".into()
    }

    fn get_application_bus_address(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn atspi_version(&self) -> String {
        "2.1".into()
    }

    #[zbus(property)]
    fn id(&self) -> i32 {
        lock_store(&self.0.store).app_id
    }

    #[zbus(property)]
    fn set_id(&self, value: i32) {
        lock_store(&self.0.store).app_id = value;
    }

    #[zbus(property)]
    fn toolkit_name(&self) -> String {
        lock_store(&self.0.store).tree.app_name.clone()
    }

    #[zbus(property)]
    fn version(&self) -> String {
        "1".into()
    }
}

struct SocketRoot;

#[interface(name = "org.a11y.atspi.Socket")]
impl SocketRoot {
    fn embed(&self, _plug: (&str, ObjectPath<'_>)) -> (String, OwnedObjectPath) {
        null_ref()
    }

    fn embedded(&self, _path: ObjectPath<'_>) {}

    fn unembed(&self, _plug: (&str, ObjectPath<'_>)) {}
}

fn empty_tree(app_name: &str) -> PublishedTree {
    PublishedTree {
        app_name: app_name.to_owned(),
        nodes: vec![PublishedNode {
            id: 0,
            parent: None,
            role: PublishedRole::Application,
            name: app_name.to_owned(),
            bounds: AccessibilityBounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            focusable: false,
            focused: false,
            editable: false,
            clickable: false,
        }],
    }
}

fn hydrate_session_bus_env() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
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

async fn register_node(
    conn: &zbus::Connection,
    store: &Arc<Mutex<Store>>,
    id: u32,
) -> Result<(), AccessibilityPublishError> {
    let path = object_path(id);
    let ctx = Ctx {
        store: Arc::clone(store),
        id,
    };
    conn.object_server()
        .at(path.as_str(), AccessibleNode(ctx.clone()))
        .await
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_export", error))?;
    conn.object_server()
        .at(path.as_str(), ComponentNode(ctx.clone()))
        .await
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_export", error))?;
    conn.object_server()
        .at(path.as_str(), ActionNode(ctx.clone()))
        .await
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_export", error))?;
    if id == 0 {
        conn.object_server()
            .at(path.as_str(), ApplicationRoot(ctx))
            .await
            .map_err(|error| AccessibilityPublishError::failed("a11y_publish_export", error))?;
        conn.object_server()
            .at(path.as_str(), SocketRoot)
            .await
            .map_err(|error| AccessibilityPublishError::failed("a11y_publish_export", error))?;
    }
    Ok(())
}

async fn serve(
    store: Arc<Mutex<Store>>,
) -> Result<AccessibilityConnection, AccessibilityPublishError> {
    hydrate_session_bus_env();
    let conn = AccessibilityConnection::new()
        .await
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_connect", error))?;
    let unique = conn
        .connection()
        .unique_name()
        .map(|name| name.as_str().to_owned())
        .ok_or_else(|| {
            AccessibilityPublishError::failed(
                "a11y_publish_connect",
                "a11y bus connection has no unique name",
            )
        })?;
    lock_store(&store).unique_name = unique.clone();

    // Fixed chrome ids 0..=5. Product snapshots reuse these paths.
    for id in 0..=5 {
        register_node(conn.connection(), &store, id).await?;
    }

    let root = ObjectPath::try_from(ROOT_PATH)
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_export", error))?;
    let socket = SocketProxy::new(conn.connection())
        .await
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_embed", error))?;
    socket
        .embed(&(&unique, root.as_ref()))
        .await
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_embed", error))?;
    Ok(conn)
}

pub(crate) struct PublisherInner {
    store: Arc<Mutex<Store>>,
}

impl PublisherInner {
    pub(crate) fn publish(&self, tree: PublishedTree) {
        let mut store = lock_store(&self.store);
        store.focused = tree
            .nodes
            .iter()
            .find(|node| node.focused)
            .map(|node| node.id);
        store.tree = tree;
    }

    pub(crate) fn set_handler(&self, handler: PublishedActionHandler) {
        lock_store(&self.store).handler = Some(handler);
    }

    pub(crate) fn set_window_handle(&self, window_handle: Option<i64>) {
        lock_store(&self.store).window_handle = window_handle;
    }
}

pub(crate) fn start(
    app_name: &str,
    window_handle: Option<i64>,
) -> Result<AccessibilityPublisher, AccessibilityPublishError> {
    let store = Arc::new(Mutex::new(Store {
        tree: empty_tree(app_name),
        window_handle,
        unique_name: String::new(),
        app_id: 0,
        focused: None,
        handler: None,
    }));
    let thread_store = Arc::clone(&store);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("agenterm-a11y-publish".into())
        .spawn(move || {
            let result = runtime().block_on(serve(thread_store));
            match result {
                Ok(conn) => {
                    let _ = ready_tx.send(Ok(()));
                    runtime().block_on(async move {
                        let _conn = conn;
                        std::future::pending::<()>().await;
                    });
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        })
        .map_err(|error| AccessibilityPublishError::failed("a11y_publish_thread", error))?;
    match ready_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(())) => Ok(AccessibilityPublisher::from_inner(PublisherInner { store })),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(AccessibilityPublishError::failed(
            "a11y_publish_timeout",
            "AT-SPI publisher did not register before its deadline",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_editable_input_sets_atspi_state_bits() {
        let node = PublishedNode {
            id: 4,
            parent: Some(1),
            role: PublishedRole::Text,
            name: "Command".into(),
            bounds: AccessibilityBounds {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            focusable: true,
            focused: true,
            editable: true,
            clickable: true,
        };
        let bits = state_bits(&node, Some(4));
        assert_ne!(bits & STATE_FOCUSABLE, 0);
        assert_ne!(bits & STATE_FOCUSED, 0);
        assert_ne!(bits & STATE_EDITABLE, 0);
        assert_ne!(bits & STATE_SHOWING, 0);
    }

    #[test]
    fn button_exposes_named_click_action() {
        let node = PublishedNode {
            id: 5,
            parent: Some(1),
            role: PublishedRole::Button,
            name: "SEND".into(),
            bounds: AccessibilityBounds {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            focusable: true,
            focused: false,
            editable: false,
            clickable: true,
        };
        let actions = actions_for(&node);
        assert_eq!(actions[0].0, "click");
        assert!(actions.iter().any(|action| action.0 == "focus"));
    }
}
