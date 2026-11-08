//! Rust conveniences for native Phlox plugin authors.
//! The generated shared-library boundary uses only the versioned C ABI.

pub use phlox_plugin_api as abi;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
};

/// Registration errors are returned to the native host as ABI statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationError(pub abi::Status);

pub type Result<T> = std::result::Result<T, RegistrationError>;

/// Short-lived node type registrar. Neither it nor the host context can be retained.
pub struct Registrar<'a> {
    host: &'a abi::HostApi,
}

impl Registrar<'_> {
    /// Advertise a node type and its JSON Schema (or metadata JSON).
    /// The host copies both strings before this call returns.
    pub fn node_type(&mut self, id: &str, schema_json: &str) -> Result<()> {
        if id.is_empty() || schema_json.is_empty() {
            return Err(RegistrationError(abi::Status::InvalidArgument));
        }
        let callback = self
            .host
            .register_node_type
            .ok_or(RegistrationError(abi::Status::InvalidArgument))?;
        let status = unsafe {
            callback(
                self.host.context,
                abi::ByteSpan {
                    data: id.as_ptr(),
                    len: id.len(),
                },
                abi::ByteSpan {
                    data: schema_json.as_ptr(),
                    len: schema_json.len(),
                },
            )
        };
        match status {
            abi::Status::Ok => Ok(()),
            other => Err(RegistrationError(other)),
        }
    }
}

/// Only invoked by the generated C entrypoint. Validates the host table before use.
///
/// # Safety
/// `host` must be null or point to a readable, correctly aligned `HostApi`
/// with at least the size of its first field; if its advertised size is large
/// enough, it must contain the complete v1 layout. It must stay alive during
/// registration.
pub unsafe fn register_from_host(
    host: *const abi::HostApi,
    register: fn(&mut Registrar<'_>) -> Result<()>,
) -> abi::Status {
    let Some(host) = NonNull::new(host.cast_mut()) else {
        return abi::Status::InvalidArgument;
    };
    let status = catch_unwind(AssertUnwindSafe(|| {
        let host = unsafe { host.as_ref() };
        if host.struct_size < std::mem::size_of::<abi::HostApi>()
            || host.abi_version != abi::ABI_VERSION
            || host.register_node_type.is_none()
        {
            return abi::Status::UnsupportedAbi;
        }
        match register(&mut Registrar { host }) {
            Ok(()) => abi::Status::Ok,
            Err(error) => error.0,
        }
    }));
    status.unwrap_or(abi::Status::InternalError)
}

/// Export the required native entrypoint from a `cdylib`.
///
/// ```ignore
/// fn register(reg: &mut phlox_plugin_sdk::Registrar<'_>) -> phlox_plugin_sdk::Result<()> {
///     reg.node_type("example/source", r#"{"type":"object"}"#)
/// }
/// phlox_plugin_sdk::export_plugin!(
///     id: "example",
///     version: "0.1.0",
///     register: register
/// );
/// ```
#[macro_export]
macro_rules! export_plugin {
    (id: $id:literal, version: $version:literal, register: $register:path $(,)?) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn phlox_plugin_entry(
            host_abi_version: u32,
            out_plugin: *mut *const $crate::abi::PluginApi,
        ) -> $crate::abi::Status {
            // The table and literal strings are immutable and live for the
            // entire library lifetime; the raw spans never change.
            struct StaticApi($crate::abi::PluginApi);
            unsafe impl Sync for StaticApi {}
            static API: StaticApi = StaticApi($crate::abi::PluginApi {
                struct_size: ::core::mem::size_of::<$crate::abi::PluginApi>(),
                abi_version: $crate::abi::ABI_VERSION,
                plugin_id: $crate::abi::ByteSpan::from_static($id.as_bytes()),
                plugin_version: $crate::abi::ByteSpan::from_static($version.as_bytes()),
                register: Some(register_entry),
            });
            unsafe extern "C" fn register_entry(
                host: *const $crate::abi::HostApi,
            ) -> $crate::abi::Status {
                unsafe { $crate::register_from_host(host, $register) }
            }
            if out_plugin.is_null() {
                return $crate::abi::Status::InvalidArgument;
            }
            unsafe {
                *out_plugin = ::core::ptr::null();
            }
            if host_abi_version != $crate::abi::ABI_VERSION {
                return $crate::abi::Status::UnsupportedAbi;
            }
            unsafe {
                *out_plugin = &API.0;
            }
            $crate::abi::Status::Ok
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    unsafe extern "C" fn capture(
        context: *mut c_void,
        id: abi::ByteSpan,
        schema: abi::ByteSpan,
    ) -> abi::Status {
        let output = unsafe { &mut *(context as *mut Vec<(String, String)>) };
        let decode = |span: abi::ByteSpan| unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(span.data, span.len))
                .unwrap()
                .to_owned()
        };
        output.push((decode(id), decode(schema)));
        abi::Status::Ok
    }

    fn example_register(registrar: &mut Registrar<'_>) -> Result<()> {
        registrar.node_type("sample/source", r#"{"type":"object"}"#)
    }

    export_plugin!(
        id: "sample",
        version: "0.1.0",
        register: example_register,
    );

    #[test]
    fn generated_entrypoint_negotiates_abi() {
        let mut table: *const abi::PluginApi = std::ptr::null();
        assert_eq!(
            unsafe { phlox_plugin_entry(abi::ABI_VERSION + 1, &mut table) },
            abi::Status::UnsupportedAbi
        );
        assert!(table.is_null());
        assert_eq!(
            unsafe { phlox_plugin_entry(abi::ABI_VERSION, &mut table) },
            abi::Status::Ok
        );
        assert!(!table.is_null());
        let plugin = unsafe { &*table };
        assert_eq!(plugin.abi_version, abi::ABI_VERSION);
        assert_eq!(plugin.struct_size, std::mem::size_of::<abi::PluginApi>());
    }

    #[test]
    fn registration_copies_node_descriptors() {
        let mut nodes = Vec::new();
        let host = abi::HostApi {
            struct_size: std::mem::size_of::<abi::HostApi>(),
            abi_version: abi::ABI_VERSION,
            context: (&mut nodes as *mut Vec<(String, String)>).cast(),
            register_node_type: Some(capture),
        };
        fn register(r: &mut Registrar<'_>) -> Result<()> {
            r.node_type("example/source", r#"{"type":"object"}"#)
        }
        let status = unsafe { register_from_host(&host, register) };
        assert_eq!(status, abi::Status::Ok);
        assert_eq!(
            nodes,
            vec![(
                "example/source".to_owned(),
                r#"{"type":"object"}"#.to_owned()
            )]
        );
    }

    #[test]
    fn rejects_missing_callback() {
        let host = abi::HostApi {
            struct_size: std::mem::size_of::<abi::HostApi>(),
            abi_version: abi::ABI_VERSION,
            context: std::ptr::null_mut(),
            register_node_type: None,
        };
        assert_eq!(
            unsafe { register_from_host(&host, |_| Ok(())) },
            abi::Status::UnsupportedAbi
        );
    }

    #[test]
    fn contains_panics_at_ffi_boundary() {
        let host = abi::HostApi {
            struct_size: std::mem::size_of::<abi::HostApi>(),
            abi_version: abi::ABI_VERSION,
            context: std::ptr::null_mut(),
            register_node_type: Some(capture),
        };
        assert_eq!(
            unsafe { register_from_host(&host, |_| panic!("plugin panic")) },
            abi::Status::InternalError
        );
    }
}
