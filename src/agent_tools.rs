//! LLM tool-table projection of [`crate::operations::OPERATION_CATALOG`].
//!
//! The hyper-control agent (`plan/design-cc-hyper-control-agent.md`) needs one
//! machine-readable answer to "what can I do to AgenTerm, and which of those
//! must a human approve first". That answer must be **derived**, never written
//! by hand: `plan/agent-human-parity-audit.md` F3/F4 record what hand-copied
//! surfaces cost — `--help` advertised `send-mouse`, which no host dispatches,
//! while hiding `ui-input`, the only pointer verb that works.
//!
//! So this module is a *projection*, in the same sense as
//! [`crate::script_catalog::entries`], which already derives the script API
//! from the same catalog. Adding one `OperationSpec` widens the agent's tool
//! table with no edit here, and `every_available_operation_is_projected`
//! fails the build if that ever stops being true.
//!
//! What this module deliberately does **not** do:
//!
//! - It does not invent an `argv` template. The catalog declares parameter
//!   *names*, not their CLI spellings (`tab` is `-t`, `instance` is
//!   positional), so a generated command line would be a fact the catalog does
//!   not hold — exactly the F3 failure mode. Callers get the typed
//!   `operation_id` plus its `script_surface`, both of which take the declared
//!   parameter names verbatim.
//! - It does not decide policy. `approval.required` marks *which* calls need a
//!   gate; who opens the gate stays with the operator, matching the
//!   `classification_only` / `authorization_policy: false` stance that
//!   `protocol-info` already publishes for the raw catalog.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::operations::{OPERATION_CATALOG, OperationClass, OperationParameterSpec, OperationSpec};

pub const AGENT_TOOL_SCHEMA_VERSION: u32 = 1;

/// Tool-name prefix. Keeps the table self-identifying when it is merged into a
/// harness that also exposes unrelated tools.
const TOOL_NAME_PREFIX: &str = "agenterm_";

/// Anthropic and OpenAI both constrain tool names to `^[a-zA-Z0-9_-]{1,64}$`,
/// which the dotted operation ids (`ui.tab.close`) do not satisfy. The slug is
/// mechanical and `tool_names_are_unique_and_wire_legal` pins both properties.
const TOOL_NAME_MAX: usize = 64;

/// The wire constraint every mainstream tool-calling API imposes on a tool
/// name. It lives here rather than in a test so the rule the projection must
/// satisfy is stated once, next to the slug that has to satisfy it.
pub fn tool_name_is_wire_legal(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= TOOL_NAME_MAX
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

/// `u32::MAX`; spelled out because JSON Schema bounds are plain numbers.
const U32_MAX: i64 = u32::MAX as i64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalGate {
    /// Observation and reversible control: the agent may call it unattended.
    None,
    /// Irreversible: ends a PTY, a server, or a window. The hyper-control
    /// agent must raise an explicit approval gate before calling it
    /// (`plan/design-cc-hyper-control-agent.md` § 1.2).
    ExplicitHumanApproval,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AgentToolApproval {
    pub required: bool,
    pub gate: ApprovalGate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// MCP `tools/list` annotation block, spelled in MCP's camelCase so the
/// projection can be forwarded without a rename pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AgentToolAnnotations {
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: bool,
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: bool,
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: bool,
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: bool,
}

/// How to actually issue the call. Every field here is copied from the
/// catalog; nothing is synthesised.
#[derive(Clone, Debug, Serialize)]
pub struct AgentToolInvocation {
    pub operation_id: &'static str,
    pub script_surface: &'static str,
    pub control_command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_action: Option<&'static str>,
    pub aliases: &'static [&'static str],
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentTool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub class: OperationClass,
    pub mutating: bool,
    pub approval: AgentToolApproval,
    pub annotations: AgentToolAnnotations,
    pub input_schema: Value,
    pub invocation: AgentToolInvocation,
    pub result_type: &'static str,
    /// The declared typed failure vocabulary. An agent that branches on these
    /// can tell "I called it wrong" from "the environment refused".
    pub errors: &'static [&'static str],
    pub events: &'static [&'static str],
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<&'static str>,
    pub since: &'static str,
}

/// The tool table the hyper-control agent should consume: every catalog
/// operation that is actually available, and nothing else.
pub fn agent_tool_catalog() -> Vec<AgentTool> {
    project_catalog(OPERATION_CATALOG, false)
}

/// Same projection, but keeps `available: false` operations with an explicit
/// `available: false` + `unavailable_reason`. For inspection and for harnesses
/// that want to *show* a greyed-out capability; never for tool dispatch.
pub fn agent_tool_catalog_including_unavailable() -> Vec<AgentTool> {
    project_catalog(OPERATION_CATALOG, true)
}

/// Catalog-agnostic core so tests can project synthetic specs — in particular
/// an unavailable one, which the shipped catalog currently has none of.
pub fn project_catalog(
    catalog: &'static [OperationSpec],
    include_unavailable: bool,
) -> Vec<AgentTool> {
    catalog
        .iter()
        .filter(|operation| include_unavailable || operation.available)
        .map(project_operation)
        .collect()
}

/// The whole document, `schema_version` first, ready to hand to a harness.
pub fn agent_tool_catalog_json(include_unavailable: bool) -> Value {
    let tools = project_catalog(OPERATION_CATALOG, include_unavailable);
    json!({
        "schema_version": AGENT_TOOL_SCHEMA_VERSION,
        "operation_catalog_schema_version": crate::operations::OPERATION_CATALOG_SCHEMA_VERSION,
        "agenterm_version": env!("CARGO_PKG_VERSION"),
        "derived_from": "src/operations.rs::OPERATION_CATALOG",
        "classification_only": true,
        "authorization_policy": false,
        "includes_unavailable": include_unavailable,
        "tools": tools,
    })
}

/// The same tools in MCP `tools/list` shape. Provided so a bridge never has to
/// re-key the projection by hand; see the module note on why nothing in this
/// crate serves it yet.
pub fn agent_tool_catalog_mcp_json(include_unavailable: bool) -> Value {
    let tools = project_catalog(OPERATION_CATALOG, include_unavailable)
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "annotations": tool.annotations,
            })
        })
        .collect::<Vec<_>>();
    json!({ "tools": tools })
}

fn project_operation(operation: &'static OperationSpec) -> AgentTool {
    let mutating = operation.class != OperationClass::Observe;
    AgentTool {
        name: tool_name(operation.id),
        title: title_for(operation),
        description: description_for(operation),
        class: operation.class,
        mutating,
        approval: approval_for(operation),
        annotations: AgentToolAnnotations {
            read_only_hint: !mutating,
            destructive_hint: operation.destructive,
            // Only meaningful for mutating tools, and no catalog field claims
            // idempotence, so the honest answer is "not declared".
            idempotent_hint: false,
            // Every operation acts on this machine's own AgenTerm instance.
            open_world_hint: false,
        },
        input_schema: input_schema_for(operation.parameters),
        invocation: AgentToolInvocation {
            operation_id: operation.id,
            script_surface: operation.script_surface,
            control_command: operation.command,
            control_action: operation.action,
            aliases: operation.aliases,
        },
        result_type: operation.result_type,
        errors: operation.errors,
        events: operation.events,
        available: operation.available,
        unavailable_reason: (!operation.available).then_some("operation_unavailable"),
        since: operation.since,
    }
}

/// `ui.tab.close` -> `agenterm_ui_tab_close`. Reversible in practice because
/// `invocation.operation_id` carries the dotted identity unchanged.
pub fn tool_name(operation_id: &str) -> String {
    let mut name = String::with_capacity(TOOL_NAME_PREFIX.len() + operation_id.len());
    name.push_str(TOOL_NAME_PREFIX);
    for character in operation_id.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character);
        } else {
            name.push('_');
        }
    }
    name
}

/// The typed identity is the title. Pairing `command` with `action` would read
/// like a command line, and it is not one: `control-center.open` has
/// `command: "control-center"` but its action `open-control-center` belongs to
/// `ui-action`. Those two facts are kept apart in `invocation`.
fn title_for(operation: &OperationSpec) -> String {
    operation.id.to_owned()
}

/// Descriptions are generated, not authored. `OperationSpec` has no prose
/// field, and adding one would mean a human sentence per operation — a
/// hand-maintained table wearing a derived table's clothes.
fn description_for(operation: &OperationSpec) -> String {
    let mut text = String::new();
    text.push_str(match operation.class {
        OperationClass::Observe => "Observe-only operation; does not change AgenTerm state. ",
        OperationClass::Control => "Control operation; changes AgenTerm state reversibly. ",
        OperationClass::Destructive => {
            "DESTRUCTIVE operation; the effect cannot be undone. Requires explicit human \
             approval before it is called. "
        }
    });
    text.push_str(&format!(
        "Typed identity `{}`; script surface `{}`; control command `{}`",
        operation.id, operation.script_surface, operation.command
    ));
    if let Some(action) = operation.action {
        text.push_str(&format!("; action `{action}`"));
    }
    text.push_str(". ");
    text.push_str(&format!("Returns `{}`.", operation.result_type));
    if !operation.errors.is_empty() {
        text.push_str(&format!(
            " Declared failures: {}.",
            operation.errors.join(", ")
        ));
    }
    if !operation.events.is_empty() {
        text.push_str(&format!(" Emits: {}.", operation.events.join(", ")));
    }
    if !operation.available {
        text.push_str(" UNAVAILABLE: this operation is declared but not shipped; do not call it.");
    }
    text
}

fn approval_for(operation: &OperationSpec) -> AgentToolApproval {
    if operation.destructive || operation.class == OperationClass::Destructive {
        AgentToolApproval {
            required: true,
            gate: ApprovalGate::ExplicitHumanApproval,
            reason: Some("destructive_operation"),
        }
    } else {
        AgentToolApproval {
            required: false,
            gate: ApprovalGate::None,
            reason: None,
        }
    }
}

fn input_schema_for(parameters: &'static [OperationParameterSpec]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for parameter in parameters {
        properties.insert(parameter.name.to_owned(), parameter_schema(parameter));
        if parameter.required {
            required.push(Value::String(parameter.name.to_owned()));
        }
    }
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "properties": Value::Object(properties),
        "required": Value::Array(required),
    })
}

/// `minimum` / `maximum` on an `OperationParameterSpec` mean **value bounds**
/// for numeric types and **byte-length bounds** for string types (`note` is
/// `0..=4096` bytes, `client_id` is `1..=128`). Collapsing both onto JSON
/// Schema `minimum` would tell an agent that a note must be a number.
fn parameter_schema(parameter: &OperationParameterSpec) -> Value {
    match value_type_shape(parameter.value_type) {
        Some(ValueShape::Number) => bounded_number("number", parameter, None, None),
        Some(ValueShape::Integer) => bounded_number("integer", parameter, None, None),
        Some(ValueShape::Uint32) => bounded_number("integer", parameter, Some(0), Some(U32_MAX)),
        // `u64::MAX` exceeds the IEEE-754 range JSON numbers round-trip, so
        // only the lower bound is declared.
        Some(ValueShape::Uint64) => bounded_number("integer", parameter, Some(0), None),
        Some(ValueShape::Text) => bounded_string(parameter, None),
        Some(ValueShape::StableTabId) => bounded_string(parameter, Some("^@[0-9]+$")),
        Some(ValueShape::SessionName) => bounded_string(parameter, None),
        None => unknown_value_type_schema(parameter),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueShape {
    Number,
    Integer,
    Uint32,
    Uint64,
    Text,
    StableTabId,
    SessionName,
}

/// Returning `None` for an unrecognised `value_type` is what lets
/// `every_declared_value_type_has_a_schema_mapping` fail loudly when someone
/// adds a type to the catalog: silently guessing `string` would ship a schema
/// that misdescribes the operation.
fn value_type_shape(value_type: &str) -> Option<ValueShape> {
    match value_type {
        "number" => Some(ValueShape::Number),
        "integer" => Some(ValueShape::Integer),
        "uint32" => Some(ValueShape::Uint32),
        "uint64" => Some(ValueShape::Uint64),
        "string" => Some(ValueShape::Text),
        "stable_tab_id" => Some(ValueShape::StableTabId),
        "session_name" => Some(ValueShape::SessionName),
        _ => None,
    }
}

fn bounded_number(
    json_type: &str,
    parameter: &OperationParameterSpec,
    default_minimum: Option<i64>,
    default_maximum: Option<i64>,
) -> Value {
    let mut schema = Map::new();
    schema.insert("type".to_owned(), Value::String(json_type.to_owned()));
    schema.insert(
        "x-agenterm-value-type".to_owned(),
        Value::String(parameter.value_type.to_owned()),
    );
    if let Some(minimum) = parameter.minimum.or(default_minimum) {
        schema.insert("minimum".to_owned(), Value::from(minimum));
    }
    if let Some(maximum) = parameter.maximum.or(default_maximum) {
        schema.insert("maximum".to_owned(), Value::from(maximum));
    }
    Value::Object(schema)
}

fn bounded_string(parameter: &OperationParameterSpec, pattern: Option<&str>) -> Value {
    let mut schema = Map::new();
    schema.insert("type".to_owned(), Value::String("string".to_owned()));
    schema.insert(
        "x-agenterm-value-type".to_owned(),
        Value::String(parameter.value_type.to_owned()),
    );
    if let Some(minimum) = parameter.minimum {
        schema.insert("minLength".to_owned(), Value::from(minimum));
    }
    if let Some(maximum) = parameter.maximum {
        schema.insert("maxLength".to_owned(), Value::from(maximum));
    }
    if let Some(pattern) = pattern {
        schema.insert("pattern".to_owned(), Value::String(pattern.to_owned()));
    }
    Value::Object(schema)
}

/// Unreachable for the shipped catalog (a test proves it). If it ever is
/// reached, the schema says "unconstrained and undescribed" rather than
/// guessing.
fn unknown_value_type_schema(parameter: &OperationParameterSpec) -> Value {
    json!({
        "x-agenterm-value-type": parameter.value_type,
        "x-agenterm-unmapped": true,
        "description": format!(
            "value_type `{}` has no JSON Schema mapping in agent_tools.rs",
            parameter.value_type
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{
        UI_TAB_CLOSE, UI_TABS_SET_WIDTH, UI_TABS_SHOW, UI_WINDOW_CLOSE_KEEP_SERVER,
        UI_WINDOW_CLOSE_STOP_SERVER, UI_WINDOW_RESIZE,
    };

    fn tool(id: &str) -> AgentTool {
        agent_tool_catalog()
            .into_iter()
            .find(|tool| tool.invocation.operation_id == id)
            .unwrap_or_else(|| panic!("{id} is missing from the agent tool table"))
    }

    /// **The point of this module.** If someone ever "helpfully" replaces the
    /// projection with a hand-written list, this fails on the next catalog
    /// entry. It is the difference between a table that widens itself and the
    /// hand-copied surfaces the audit's F3/F4 record.
    #[test]
    fn every_available_operation_is_projected() {
        let tools = agent_tool_catalog();
        let missing: Vec<&str> = OPERATION_CATALOG
            .iter()
            .filter(|operation| operation.available)
            .map(|operation| operation.id)
            .filter(|id| !tools.iter().any(|tool| tool.invocation.operation_id == *id))
            .collect();
        assert!(
            missing.is_empty(),
            "available operations absent from the agent tool table: {missing:?}"
        );
        assert_eq!(
            tools.len(),
            OPERATION_CATALOG
                .iter()
                .filter(|operation| operation.available)
                .count(),
            "the tool table must be exactly the available catalog, not a subset or a superset"
        );
    }

    /// The reverse direction: no tool may exist without a catalog entry
    /// behind it. A tool an agent can select but no dispatcher implements is
    /// the `send-mouse` accident (audit F3) in tool-calling clothes.
    #[test]
    fn no_tool_exists_without_a_catalog_entry() {
        for tool in agent_tool_catalog() {
            let operation = crate::operations::operation_by_id(tool.invocation.operation_id)
                .unwrap_or_else(|| panic!("{} has no catalog entry", tool.name));
            assert_eq!(tool.result_type, operation.result_type);
            assert_eq!(tool.errors, operation.errors);
            assert_eq!(tool.events, operation.events);
            assert_eq!(tool.since, operation.since);
            assert_eq!(tool.class, operation.class);
        }
    }

    /// Honesty gate: an operation the catalog marks unavailable must not be
    /// selectable. The shipped catalog has none today, so the guard runs
    /// against a synthetic spec rather than silently passing forever.
    #[test]
    fn unavailable_operations_never_reach_the_default_table() {
        assert!(
            agent_tool_catalog().iter().all(|tool| tool.available),
            "the default table must contain only available operations"
        );

        static SYNTHETIC: &[OperationSpec] = &[OperationSpec {
            id: "test.unavailable",
            script_surface: "fleet.test.unavailable",
            class: OperationClass::Control,
            command: "ui-action",
            action: Some("test-unavailable"),
            aliases: &[],
            parameters: &[],
            result_type: "ui_snapshot",
            errors: &[],
            events: &[],
            destructive: false,
            available: false,
            since: "0.0.0",
        }];

        assert!(
            project_catalog(SYNTHETIC, false).is_empty(),
            "an unavailable operation must be dropped from the default table"
        );
        let inspected = project_catalog(SYNTHETIC, true);
        assert_eq!(inspected.len(), 1);
        assert!(!inspected[0].available);
        assert_eq!(
            inspected[0].unavailable_reason,
            Some("operation_unavailable")
        );
        assert!(
            inspected[0].description.contains("UNAVAILABLE"),
            "an unavailable tool must say so in prose an LLM reads: {}",
            inspected[0].description
        );
    }

    /// The approval gate `plan/design-cc-hyper-control-agent.md` § 1.2
    /// requires: a table that cannot tell `ui.tabs.show` from `ui.tab.close`
    /// gives the operator nothing to gate on.
    #[test]
    fn destructive_operations_carry_an_explicit_approval_gate() {
        for id in [UI_TAB_CLOSE, UI_WINDOW_CLOSE_STOP_SERVER, "server.kill"] {
            let tool = tool(id);
            assert!(tool.approval.required, "{id} must require approval");
            assert_eq!(tool.approval.gate, ApprovalGate::ExplicitHumanApproval);
            assert_eq!(tool.approval.reason, Some("destructive_operation"));
            assert!(tool.annotations.destructive_hint);
            assert!(!tool.annotations.read_only_hint);
            assert!(tool.mutating);
        }
        for id in [UI_TABS_SHOW, UI_WINDOW_CLOSE_KEEP_SERVER] {
            let tool = tool(id);
            assert!(!tool.approval.required, "{id} must not require approval");
            assert_eq!(tool.approval.gate, ApprovalGate::None);
            assert!(!tool.annotations.destructive_hint);
        }
        assert_eq!(
            agent_tool_catalog()
                .iter()
                .filter(|tool| tool.approval.required)
                .count(),
            OPERATION_CATALOG
                .iter()
                .filter(|operation| operation.available && operation.destructive)
                .count(),
            "the gated set must be exactly the catalog's destructive set"
        );
    }

    /// Observe / Control / Destructive must survive the projection, otherwise
    /// an agent cannot plan "look before you leap".
    #[test]
    fn observation_and_mutation_stay_distinguishable() {
        let snapshot = tool("ui.snapshot");
        assert_eq!(snapshot.class, OperationClass::Observe);
        assert!(snapshot.annotations.read_only_hint);
        assert!(!snapshot.mutating);

        let set_width = tool(UI_TABS_SET_WIDTH);
        assert_eq!(set_width.class, OperationClass::Control);
        assert!(!set_width.annotations.read_only_hint);
        assert!(set_width.mutating);
        assert!(!set_width.approval.required);
    }

    /// A `value_type` with no mapping would ship a schema that misdescribes
    /// the operation, so adding one to the catalog must break this test rather
    /// than leak a guess into the tool table.
    #[test]
    fn every_declared_value_type_has_a_schema_mapping() {
        let unmapped: Vec<&str> = OPERATION_CATALOG
            .iter()
            .flat_map(|operation| operation.parameters.iter())
            .map(|parameter| parameter.value_type)
            .filter(|value_type| value_type_shape(value_type).is_none())
            .collect();
        assert!(
            unmapped.is_empty(),
            "value_types without a JSON Schema mapping in agent_tools.rs: {unmapped:?}"
        );
    }

    #[test]
    fn parameter_bounds_and_requiredness_reach_the_schema() {
        let resize = tool(UI_WINDOW_RESIZE).input_schema;
        assert_eq!(resize["type"], "object");
        assert_eq!(resize["additionalProperties"], false);
        assert_eq!(resize["properties"]["width"]["type"], "integer");
        let declared = crate::operations::operation_by_id(UI_WINDOW_RESIZE).unwrap();
        let width = declared
            .parameters
            .iter()
            .find(|parameter| parameter.name == "width")
            .unwrap();
        assert_eq!(
            resize["properties"]["width"]["minimum"],
            width.minimum.unwrap()
        );
        assert_eq!(
            resize["properties"]["width"]["maximum"],
            width.maximum.unwrap()
        );
        assert_eq!(
            resize["required"],
            serde_json::json!(["width", "height"]),
            "both resize extents are required"
        );

        // String bounds are byte lengths, not numeric bounds.
        let note = tool("tabs.set-note").input_schema;
        assert_eq!(note["properties"]["note"]["type"], "string");
        assert_eq!(
            note["properties"]["note"]["maxLength"],
            crate::operations::TAB_NOTE_MAX_BYTES as i64
        );
        assert!(note["properties"]["note"].get("maximum").is_none());
        assert_eq!(
            note["properties"]["tab"]["pattern"], "^@[0-9]+$",
            "stable tab ids must be shaped so an agent cannot pass a title"
        );

        // uint32 gains its implicit range even when the spec leaves it open.
        let hello = tool("ui.hello").input_schema;
        assert_eq!(hello["properties"]["minimum"]["type"], "integer");
        assert_eq!(hello["properties"]["minimum"]["maximum"], U32_MAX);
        assert_eq!(hello["properties"]["client_id"]["maxLength"], 128);

        // Optional parameters must not be forced on the agent.
        let select = tool("ui.tab.select").input_schema;
        assert_eq!(select["required"], serde_json::json!([]));
        assert!(select["properties"]["tab"].is_object());
    }

    #[test]
    fn tool_names_are_unique_and_wire_legal() {
        let tools = agent_tool_catalog_including_unavailable();
        let mut seen = std::collections::BTreeSet::new();
        for tool in &tools {
            assert!(
                seen.insert(tool.name.clone()),
                "duplicate tool name {}",
                tool.name
            );
            assert!(
                tool_name_is_wire_legal(&tool.name),
                "{} is not a legal tool name (1..={TOOL_NAME_MAX} of [A-Za-z0-9_-])",
                tool.name
            );
            assert!(tool.name.starts_with(TOOL_NAME_PREFIX));
        }
        assert_eq!(tool_name("ui.tab.close"), "agenterm_ui_tab_close");
        assert_eq!(
            tool_name("ui.window-close.stop-server-and-exit"),
            "agenterm_ui_window_close_stop_server_and_exit"
        );
    }

    #[test]
    fn document_and_mcp_shapes_are_derived_from_the_same_tools() {
        let document = agent_tool_catalog_json(false);
        assert_eq!(document["schema_version"], AGENT_TOOL_SCHEMA_VERSION);
        assert_eq!(document["classification_only"], true);
        assert_eq!(document["authorization_policy"], false);
        let tools = document["tools"].as_array().unwrap();
        assert_eq!(tools.len(), agent_tool_catalog().len());

        let mcp = agent_tool_catalog_mcp_json(false);
        let mcp_tools = mcp["tools"].as_array().unwrap();
        assert_eq!(mcp_tools.len(), tools.len());
        assert_eq!(mcp_tools[0]["name"], tools[0]["name"]);
        assert!(mcp_tools[0]["inputSchema"].is_object());
        assert!(mcp_tools[0]["annotations"]["readOnlyHint"].is_boolean());
    }

    /// The typed failure vocabulary is what lets an agent distinguish "I
    /// called it wrong" from "the environment refused"; dropping it in the
    /// projection would flatten both into an opaque error string.
    #[test]
    fn declared_failures_are_visible_to_the_model() {
        let paste = tool("terminal.paste");
        assert!(paste.errors.contains(&"terminal_paste_unsupported"));
        assert!(
            paste.description.contains("terminal_paste_unsupported"),
            "declared failures must appear in the prose the model reads: {}",
            paste.description
        );
        assert!(paste.description.contains("terminal.pasted"));
    }
}
