//! Discovery-only native plugin host. Loaded libraries stay alive until drop.
//!
//! Loading native code is inherently unsafe: only load trusted plugins.

use libloading::{Library, Symbol};
use phlox_plugin_api as abi;
use serde_json::Value;
use std::{ffi::c_void, path::Path};

#[derive(Debug, Clone, PartialEq)]
pub struct NodeType {
    pub id: String,
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    pub id: String,
    pub version: String,
    pub nodes: Vec<NodeType>,
}

#[derive(Debug)]
pub enum LoadError {
    Library(libloading::Error),
    InvalidPlugin(&'static str),
    AbiStatus(abi::Status),
    Registration(String),
}

impl From<libloading::Error> for LoadError {
    fn from(value: libloading::Error) -> Self {
        Self::Library(value)
    }
}

/// The library must remain mapped while plugin-owned data/function pointers exist.
pub struct LoadedPlugin {
    // Drop plugin-owned strings and data before unloading the library.
    pub info: PluginInfo,
    _library: Library,
}

struct Registration {
    nodes: Vec<NodeType>,
    error: Option<String>,
}

/// Called synchronously by a trusted plugin during registration.
///
/// # Safety
/// Context is the valid temporary Registration provided by load(), and byte
/// spans are readable for the duration of the callback. Native plugins must
/// honor those requirements.
unsafe extern "C" fn register_node(
    context: *mut c_void,
    id: abi::ByteSpan,
    schema: abi::ByteSpan,
) -> abi::Status {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() {
            return abi::Status::InvalidArgument;
        }
        let state = unsafe { &mut *(context.cast::<Registration>()) };
        let read = |span: abi::ByteSpan| -> Result<&[u8], &'static str> {
            if span.data.is_null() {
                return Err("null byte span");
            }
            // This callback is synchronous; producer keeps the memory alive.
            Ok(unsafe { std::slice::from_raw_parts(span.data, span.len) })
        };
        let parsed = (|| -> Result<NodeType, String> {
            let id = std::str::from_utf8(read(id)?)
                .map_err(|e| e.to_string())?
                .to_owned();
            if id.trim().is_empty() {
                return Err("empty node ID".into());
            }
            if state.nodes.iter().any(|n| n.id == id) {
                return Err("duplicate node ID".into());
            }
            let schema: Value = serde_json::from_slice(read(schema)?).map_err(|e| e.to_string())?;
            if !schema.is_object() {
                return Err("node schema must be a JSON object".into());
            }
            Ok(NodeType { id, schema })
        })();
        match parsed {
            Ok(node) => {
                state.nodes.push(node);
                abi::Status::Ok
            }
            Err(error) => {
                state.error = Some(error);
                abi::Status::InvalidArgument
            }
        }
    }));
    result.unwrap_or(abi::Status::InternalError)
}

/// Load a trusted Linux shared library and register its node descriptors.
///
/// # Safety
/// Native plugin initializers, entrypoints, and callbacks execute arbitrary
/// code in the current process. The caller must trust the binary and ensure
/// it respects the ABI contract; malformed pointers cannot be made safe.
pub unsafe fn load(path: impl AsRef<Path>) -> Result<LoadedPlugin, LoadError> {
    let library = unsafe { Library::new(path.as_ref())? };
    let info = {
        let entry: Symbol<'_, abi::PluginEntry> = unsafe { library.get(abi::ENTRY_SYMBOL)? };
        let mut api_ptr = std::ptr::null();
        let result = unsafe { entry(abi::ABI_VERSION, &mut api_ptr) };
        if result != abi::Status::Ok {
            return Err(LoadError::AbiStatus(result));
        }
        if api_ptr.is_null() {
            return Err(LoadError::InvalidPlugin("null plugin table"));
        }
        let api = unsafe { &*api_ptr };
        if api.struct_size < std::mem::size_of::<abi::PluginApi>()
            || api.abi_version != abi::ABI_VERSION
        {
            return Err(LoadError::InvalidPlugin("invalid ABI table"));
        }
        let decode = |span: abi::ByteSpan| -> Result<String, LoadError> {
            if span.data.is_null() {
                return Err(LoadError::InvalidPlugin("null metadata"));
            }
            let value = unsafe { std::slice::from_raw_parts(span.data, span.len) };
            let value = std::str::from_utf8(value)
                .map_err(|_| LoadError::InvalidPlugin("invalid UTF-8 metadata"))?;
            if value.trim().is_empty() {
                return Err(LoadError::InvalidPlugin("empty plugin metadata"));
            }
            Ok(value.to_owned())
        };
        let id = decode(api.plugin_id)?;
        let version = decode(api.plugin_version)?;
        let register = api
            .register
            .ok_or(LoadError::InvalidPlugin("missing register callback"))?;
        let mut state = Registration {
            nodes: Vec::new(),
            error: None,
        };
        let host = abi::HostApi {
            struct_size: std::mem::size_of::<abi::HostApi>(),
            abi_version: abi::ABI_VERSION,
            context: (&mut state as *mut Registration).cast(),
            register_node_type: Some(register_node),
        };
        let result = unsafe { register(&host) };
        if let Some(error) = state.error {
            return Err(LoadError::Registration(error));
        }
        if result != abi::Status::Ok {
            return Err(LoadError::AbiStatus(result));
        }
        PluginInfo {
            id,
            version,
            nodes: state.nodes,
        }
    };
    Ok(LoadedPlugin {
        info,
        _library: library,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_rejects_duplicate_names() {
        let mut state = Registration {
            nodes: Vec::new(),
            error: None,
        };
        let context = (&mut state as *mut Registration).cast();
        let id = abi::ByteSpan::from_static(b"example/source");
        let schema = abi::ByteSpan::from_static(br#"{"type":"object"}"#);
        assert_eq!(
            unsafe { register_node(context, id, schema) },
            abi::Status::Ok
        );
        assert_eq!(
            unsafe { register_node(context, id, schema) },
            abi::Status::InvalidArgument
        );
        assert_eq!(state.nodes.len(), 1);
    }
}

pub mod registry;

/// Synchronous proof-of-concept execution support.
/// A plugin lacking the optional execution symbol is discovery-only.
impl LoadedPlugin {
    pub fn poll(&self, node_type: &str) -> Result<Vec<(String, serde_json::Value)>, LoadError> {
        struct Emitted {
            values: Vec<(String, serde_json::Value)>,
        }
        unsafe extern "C" fn emit(
            context: *mut c_void,
            port: abi::ByteSpan,
            json: abi::ByteSpan,
        ) -> abi::Status {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if context.is_null() || port.data.is_null() || json.data.is_null() {
                    return abi::Status::InvalidArgument;
                }
                let buffer = unsafe { &mut *(context as *mut Emitted) };
                let port_bytes = unsafe { std::slice::from_raw_parts(port.data, port.len) };
                let json_bytes = unsafe { std::slice::from_raw_parts(json.data, json.len) };
                let Ok(port) = std::str::from_utf8(port_bytes) else {
                    return abi::Status::InvalidArgument;
                };
                let Ok(value) = serde_json::from_slice(json_bytes) else {
                    return abi::Status::InvalidArgument;
                };
                buffer.values.push((port.to_owned(), value));
                abi::Status::Ok
            }));
            result.unwrap_or(abi::Status::InternalError)
        }
        let entry: Symbol<'_, abi::ExecutionEntry> =
            unsafe { self._library.get(abi::EXECUTION_SYMBOL)? };
        let table = unsafe { entry() };
        if table.is_null() {
            return Err(LoadError::InvalidPlugin("missing execution table"));
        }
        let table = unsafe { &*table };
        if table.struct_size < std::mem::size_of::<abi::ExecutionApi>() {
            return Err(LoadError::InvalidPlugin("invalid execution table size"));
        }
        let poll = table
            .poll
            .ok_or(LoadError::InvalidPlugin("missing source callback"))?;
        let mut buffer = Emitted { values: Vec::new() };
        let host = abi::OutputApi {
            context: (&mut buffer as *mut Emitted).cast(),
            emit: Some(emit),
        };
        let result = unsafe {
            poll(
                abi::ByteSpan {
                    data: node_type.as_ptr(),
                    len: node_type.len(),
                },
                &host,
            )
        };
        if result != abi::Status::Ok {
            return Err(LoadError::AbiStatus(result));
        }
        Ok(buffer.values)
    }

    pub fn consume(&self, node_type: &str, value: &serde_json::Value) -> Result<(), LoadError> {
        let entry: Symbol<'_, abi::ExecutionEntry> =
            unsafe { self._library.get(abi::EXECUTION_SYMBOL)? };
        let table = unsafe { entry() };
        if table.is_null() {
            return Err(LoadError::InvalidPlugin("missing execution table"));
        }
        let table = unsafe { &*table };
        if table.struct_size < std::mem::size_of::<abi::ExecutionApi>() {
            return Err(LoadError::InvalidPlugin("invalid execution table size"));
        }
        let consume = table
            .consume
            .ok_or(LoadError::InvalidPlugin("missing sink callback"))?;
        let json = serde_json::to_vec(value)
            .map_err(|_| LoadError::InvalidPlugin("invalid message JSON"))?;
        let result = unsafe {
            consume(
                abi::ByteSpan {
                    data: node_type.as_ptr(),
                    len: node_type.len(),
                },
                abi::ByteSpan {
                    data: json.as_ptr(),
                    len: json.len(),
                },
            )
        };
        if result != abi::Status::Ok {
            return Err(LoadError::AbiStatus(result));
        }
        Ok(())
    }
}

impl LoadedPlugin {
    /// Transform a received JSON value into zero or more port-tagged outputs.
    pub fn process(
        &self,
        node_type: &str,
        input_port: &str,
        value: &serde_json::Value,
    ) -> Result<Vec<(String, serde_json::Value)>, LoadError> {
        struct Output(Vec<(String, serde_json::Value)>);
        unsafe extern "C" fn emit(
            ctx: *mut c_void,
            port: abi::ByteSpan,
            data: abi::ByteSpan,
        ) -> abi::Status {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if ctx.is_null() || port.data.is_null() || data.data.is_null() {
                    return abi::Status::InvalidArgument;
                }
                let Ok(name) =
                    std::str::from_utf8(unsafe { std::slice::from_raw_parts(port.data, port.len) })
                else {
                    return abi::Status::InvalidArgument;
                };
                let Ok(value) = serde_json::from_slice(unsafe {
                    std::slice::from_raw_parts(data.data, data.len)
                }) else {
                    return abi::Status::InvalidArgument;
                };
                unsafe { &mut *(ctx as *mut Output) }
                    .0
                    .push((name.to_owned(), value));
                abi::Status::Ok
            }));
            result.unwrap_or(abi::Status::InternalError)
        }
        let entry: Symbol<'_, abi::ProcessEntry> =
            unsafe { self._library.get(abi::PROCESS_SYMBOL)? };
        let ptr = unsafe { entry() };
        if ptr.is_null() {
            return Err(LoadError::InvalidPlugin("null process table"));
        }
        let api = unsafe { &*ptr };
        if api.struct_size < std::mem::size_of::<abi::ProcessApi>() {
            return Err(LoadError::InvalidPlugin("short process table"));
        }
        let callback = api
            .process
            .ok_or(LoadError::InvalidPlugin("no processor callback"))?;
        let json =
            serde_json::to_vec(value).map_err(|_| LoadError::InvalidPlugin("invalid JSON"))?;
        let mut output = Output(Vec::new());
        let host = abi::OutputApi {
            context: (&mut output as *mut Output).cast(),
            emit: Some(emit),
        };
        let span = |v: &str| abi::ByteSpan {
            data: v.as_ptr(),
            len: v.len(),
        };
        let status = unsafe {
            callback(
                span(node_type),
                span(input_port),
                abi::ByteSpan {
                    data: json.as_ptr(),
                    len: json.len(),
                },
                &host,
            )
        };
        if status != abi::Status::Ok {
            return Err(LoadError::AbiStatus(status));
        }
        Ok(output.0)
    }
}
