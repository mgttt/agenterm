//! Bounded Chassis-L2 dispatch from bytecode capability names to Host ABI v3.
//!
//! An app declaration is a compatibility closure, not an Agent permission or
//! sandbox policy. Calls outside that closure fail before the host callback.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

const HOST_ABI_SCHEMA: u32 = 2;
const HOST_ABI_VERSION: u32 = 3;
const MAX_CAPABILITY_NAME_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    InvalidAbi(String),
    UnknownCapability(String),
    UndeclaredCapability(String),
    InvalidParameters(String),
    ResultTooLarge { actual: usize, max: usize },
    VmResult(String),
    Host(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAbi(reason) => write!(f, "invalid L2 host ABI: {reason}"),
            Self::UnknownCapability(name) => write!(f, "unknown L2 capability `{name}`"),
            Self::UndeclaredCapability(name) => {
                write!(f, "L3 app did not declare capability `{name}`")
            }
            Self::InvalidParameters(reason) => write!(f, "invalid host parameters: {reason}"),
            Self::ResultTooLarge { actual, max } => {
                write!(f, "host result is {actual} UTF-8 bytes; max is {max}")
            }
            Self::VmResult(reason) => write!(f, "invalid L2 VM host result: {reason}"),
            Self::Host(reason) => write!(f, "L2 host callback failed: {reason}"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// Product-host callback boundary. A validated dispatch invokes this once.
pub trait HostCallback {
    fn call(&mut self, capability: &str, parameters: &Value) -> Result<Value, String>;
}

/// ABI resolver plus one app's declared compatibility closure.
pub struct Dispatcher<H> {
    aliases: BTreeMap<String, String>,
    capabilities: BTreeMap<String, CapabilityContract>,
    signatures: BTreeMap<String, Signature>,
    bounds: BTreeMap<String, NumericBound>,
    declared: BTreeSet<String>,
    maximum_result_bytes: usize,
    host: H,
}

impl<H: HostCallback> Dispatcher<H> {
    /// Build a dispatcher from the replaceable schema-2, Host ABI v3 artifact.
    pub fn from_host_abi_json(
        raw: &str,
        app_declarations: &[String],
        host: H,
    ) -> Result<Self, DispatchError> {
        let abi: HostAbiDocument =
            serde_json::from_str(raw).map_err(|err| DispatchError::InvalidAbi(err.to_string()))?;
        validate_document(&abi)?;

        let mut aliases = BTreeMap::new();
        let mut capabilities = BTreeMap::new();
        for capability in abi.capabilities {
            validate_name(&capability.id)?;
            if capability.host_abi != abi.version {
                return Err(DispatchError::InvalidAbi(format!(
                    "capability `{}` requires Host ABI {}, document is {}",
                    capability.id, capability.host_abi, abi.version
                )));
            }
            if !abi.signatures.contains_key(&capability.signature) {
                return Err(DispatchError::InvalidAbi(format!(
                    "capability `{}` names unknown signature `{}`",
                    capability.id, capability.signature
                )));
            }
            insert_alias(&mut aliases, &capability.id, &capability.id)?;
            if let Some(facade) = &capability.facade {
                validate_name(facade)?;
                insert_alias(&mut aliases, facade, &capability.id)?;
            }
            let id = capability.id.clone();
            if capabilities.insert(id.clone(), capability).is_some() {
                return Err(DispatchError::InvalidAbi(format!(
                    "duplicate capability `{id}`"
                )));
            }
        }
        if capabilities.is_empty() {
            return Err(DispatchError::InvalidAbi(
                "capability catalog is empty".to_string(),
            ));
        }

        validate_signatures(&abi.signatures, &abi.bounds)?;
        let mut declared = BTreeSet::new();
        for name in app_declarations {
            let canonical = aliases
                .get(name)
                .ok_or_else(|| DispatchError::UnknownCapability(name.clone()))?;
            declared.insert(canonical.clone());
        }

        Ok(Self {
            aliases,
            capabilities,
            signatures: abi.signatures,
            bounds: abi.bounds,
            declared,
            maximum_result_bytes: abi.wire.response.maximum_utf8_bytes,
            host,
        })
    }

    /// Resolve and dispatch one call using its exact v3 parameter signature.
    /// Invalid input never reaches the callback; valid input reaches it once.
    pub fn dispatch(
        &mut self,
        requested_name: &str,
        parameters: &Value,
    ) -> Result<Value, DispatchError> {
        let canonical = self
            .aliases
            .get(requested_name)
            .ok_or_else(|| DispatchError::UnknownCapability(requested_name.to_string()))?;
        if !self.declared.contains(canonical) {
            return Err(DispatchError::UndeclaredCapability(canonical.clone()));
        }
        let capability = self
            .capabilities
            .get(canonical)
            .expect("validated capability table");
        let signature = self
            .signatures
            .get(&capability.signature)
            .expect("validated signature reference");
        validate_parameters(parameters, &signature.parameters, &self.bounds)?;

        let result = self
            .host
            .call(canonical, parameters)
            .map_err(DispatchError::Host)?;
        let result_bytes = serde_json::to_vec(&result)
            .map_err(|err| DispatchError::Host(err.to_string()))?
            .len();
        if result_bytes > self.maximum_result_bytes {
            return Err(DispatchError::ResultTooLarge {
                actual: result_bytes,
                max: self.maximum_result_bytes,
            });
        }
        Ok(result)
    }

    pub fn into_host(self) -> H {
        self.host
    }
}

impl<H: HostCallback> crate::vm::CapHost for Dispatcher<H> {
    fn call(&mut self, capability: &str) -> Result<i64, String> {
        let value = self
            .dispatch(capability, &serde_json::json!({}))
            .map_err(|err| err.to_string())?;
        value.as_i64().ok_or_else(|| {
            DispatchError::VmResult("expected one signed integer".to_string()).to_string()
        })
    }
}

#[derive(Deserialize)]
struct HostAbiDocument {
    schema: u32,
    version: u32,
    wire: WireContract,
    bounds: BTreeMap<String, NumericBound>,
    signatures: BTreeMap<String, Signature>,
    capabilities: Vec<CapabilityContract>,
}

#[derive(Deserialize)]
struct WireContract {
    response: ResponseContract,
}

#[derive(Deserialize)]
struct ResponseContract {
    maximum_utf8_bytes: usize,
}

#[derive(Deserialize)]
struct CapabilityContract {
    id: String,
    #[serde(default)]
    facade: Option<String>,
    host_abi: u32,
    signature: String,
}

#[derive(Deserialize)]
struct Signature {
    parameters: ParameterObject,
}

#[derive(Deserialize)]
struct ParameterObject {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    required: Vec<String>,
    additional_properties: bool,
    #[serde(default)]
    properties: BTreeMap<String, PropertyContract>,
}

#[derive(Deserialize)]
struct PropertyContract {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    bound: Option<String>,
    #[serde(default)]
    minimum: Option<i64>,
    #[serde(default)]
    maximum: Option<i64>,
    #[serde(default)]
    minimum_utf8_bytes: Option<usize>,
    #[serde(default)]
    maximum_utf8_bytes: Option<usize>,
}

#[derive(Deserialize)]
struct NumericBound {
    minimum: i64,
    maximum: i64,
}

fn validate_document(abi: &HostAbiDocument) -> Result<(), DispatchError> {
    if abi.schema != HOST_ABI_SCHEMA || abi.version != HOST_ABI_VERSION {
        return Err(DispatchError::InvalidAbi(format!(
            "unsupported schema {} / Host ABI {}; expected {HOST_ABI_SCHEMA} / {HOST_ABI_VERSION}",
            abi.schema, abi.version
        )));
    }
    if abi.wire.response.maximum_utf8_bytes == 0 {
        return Err(DispatchError::InvalidAbi(
            "response maximum_utf8_bytes must be positive".to_string(),
        ));
    }
    Ok(())
}

fn validate_signatures(
    signatures: &BTreeMap<String, Signature>,
    bounds: &BTreeMap<String, NumericBound>,
) -> Result<(), DispatchError> {
    for (name, signature) in signatures {
        let parameters = &signature.parameters;
        if parameters.kind != "object" || parameters.additional_properties {
            return Err(DispatchError::InvalidAbi(format!(
                "signature `{name}` must be a closed object"
            )));
        }
        for required in &parameters.required {
            if !parameters.properties.contains_key(required) {
                return Err(DispatchError::InvalidAbi(format!(
                    "signature `{name}` requires unknown property `{required}`"
                )));
            }
        }
        for (property, contract) in &parameters.properties {
            if let Some(bound) = &contract.bound
                && !bounds.contains_key(bound)
            {
                return Err(DispatchError::InvalidAbi(format!(
                    "signature `{name}` property `{property}` names unknown bound `{bound}`"
                )));
            }
        }
    }
    Ok(())
}

fn validate_parameters(
    value: &Value,
    contract: &ParameterObject,
    bounds: &BTreeMap<String, NumericBound>,
) -> Result<(), DispatchError> {
    let object = value
        .as_object()
        .ok_or_else(|| DispatchError::InvalidParameters("expected object".to_string()))?;
    for required in &contract.required {
        if !object.contains_key(required) {
            return Err(DispatchError::InvalidParameters(format!(
                "missing required property `{required}`"
            )));
        }
    }
    for (name, value) in object {
        let property = contract.properties.get(name).ok_or_else(|| {
            DispatchError::InvalidParameters(format!("unexpected property `{name}`"))
        })?;
        validate_property(name, value, property, bounds)?;
    }
    Ok(())
}

fn validate_property(
    name: &str,
    value: &Value,
    contract: &PropertyContract,
    bounds: &BTreeMap<String, NumericBound>,
) -> Result<(), DispatchError> {
    match contract.kind.as_str() {
        "string" | "stable_tab_id" => {
            let text = value.as_str().ok_or_else(|| {
                DispatchError::InvalidParameters(format!("property `{name}` must be a string"))
            })?;
            let named_bound = contract
                .bound
                .as_ref()
                .map(|name| bounds.get(name).expect("validated bound reference"));
            let minimum = contract.minimum_utf8_bytes.unwrap_or_else(|| {
                named_bound
                    .and_then(|bound| usize::try_from(bound.minimum).ok())
                    .unwrap_or(0)
            });
            let maximum = contract.maximum_utf8_bytes.unwrap_or_else(|| {
                named_bound
                    .and_then(|bound| usize::try_from(bound.maximum).ok())
                    .unwrap_or(usize::MAX)
            });
            if text.len() < minimum || text.len() > maximum {
                return Err(DispatchError::InvalidParameters(format!(
                    "property `{name}` UTF-8 size is outside {minimum}..={maximum}"
                )));
            }
        }
        "integer" | "uint32" | "uint64" => {
            let number = value.as_i64().ok_or_else(|| {
                DispatchError::InvalidParameters(format!("property `{name}` must be an integer"))
            })?;
            if contract.kind.starts_with('u') && number < 0 {
                return Err(DispatchError::InvalidParameters(format!(
                    "property `{name}` must be unsigned"
                )));
            }
            let (minimum, maximum) = if let Some(bound) = &contract.bound {
                let bound = bounds.get(bound).expect("validated bound reference");
                (bound.minimum, bound.maximum)
            } else {
                (
                    contract.minimum.unwrap_or(i64::MIN),
                    contract.maximum.unwrap_or(i64::MAX),
                )
            };
            if number < minimum || number > maximum {
                return Err(DispatchError::InvalidParameters(format!(
                    "property `{name}` is outside {minimum}..={maximum}"
                )));
            }
        }
        "number" => {
            if !value.is_number() {
                return Err(DispatchError::InvalidParameters(format!(
                    "property `{name}` must be a number"
                )));
            }
        }
        other => {
            return Err(DispatchError::InvalidParameters(format!(
                "property `{name}` has unsupported ABI type `{other}`"
            )));
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), DispatchError> {
    if name.is_empty() || name.len() > MAX_CAPABILITY_NAME_BYTES {
        return Err(DispatchError::InvalidAbi(format!(
            "capability name must be 1..={MAX_CAPABILITY_NAME_BYTES} bytes"
        )));
    }
    Ok(())
}

fn insert_alias(
    aliases: &mut BTreeMap<String, String>,
    alias: &str,
    canonical: &str,
) -> Result<(), DispatchError> {
    if let Some(previous) = aliases.insert(alias.to_string(), canonical.to_string())
        && previous != canonical
    {
        return Err(DispatchError::InvalidAbi(format!(
            "capability alias `{alias}` resolves to both `{previous}` and `{canonical}`"
        )));
    }
    Ok(())
}
