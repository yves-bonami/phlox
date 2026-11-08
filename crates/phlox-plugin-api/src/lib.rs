//! Stable, minimal native plugin discovery ABI (version 1).
//!
//! Only C-compatible layouts cross the dynamic-library boundary. No Rust
//! strings, trait objects, allocations, or unwinding may cross it.

use core::ffi::c_void;

/// Major ABI version. Incompatible layout changes require a new major ABI.
pub const ABI_VERSION: u32 = 1;
/// Symbol exported by every native plugin. Its signature is `PluginEntry`.
pub const ENTRY_SYMBOL: &[u8] = b"phlox_plugin_entry\0";

/// Non-owning UTF-8 byte span; valid only for the lifetime specified by the API.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ByteSpan {
    pub data: *const u8,
    pub len: usize,
}

/// Status of operations across the ABI. All other values are invalid.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    UnsupportedAbi = 1,
    InvalidArgument = 2,
    InternalError = 3,
}

/// Host functions made available during plugin discovery.
/// Plugin may only access fields within `struct_size`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostApi {
    pub struct_size: usize,
    pub abi_version: u32,
    /// Host-owned user data; passed back unchanged to the callback.
    pub context: *mut c_void,
    /// Host copies the passed bytes before returning; the plugin retains ownership.
    /// The callback MUST NOT unwind.
    pub register_node_type: Option<
        unsafe extern "C" fn(context: *mut c_void, id: ByteSpan, schema_json: ByteSpan) -> Status,
    >,
}

/// Plugin interface returned from its entrypoint. The plugin owns this table
/// and must keep it alive for as long as the shared library is loaded.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginApi {
    pub struct_size: usize,
    pub abi_version: u32,
    /// Borrowed UTF-8 plugin ID, stable until library unload.
    pub plugin_id: ByteSpan,
    /// Borrowed UTF-8 plugin version, stable until library unload.
    pub plugin_version: ByteSpan,
    /// Plugin registers node types through the host callback; may be called
    /// once during discovery. MUST NOT unwind.
    pub register: Option<unsafe extern "C" fn(host: *const HostApi) -> Status>,
}

/// Plugin-owned entrypoint; `out_plugin` receives a pointer to a static table.
/// Returns UnsupportedAbi for incompatible host ABI versions.
/// All pointers are valid only while the plugin library remains loaded.
pub type PluginEntry =
    unsafe extern "C" fn(host_abi_version: u32, out_plugin: *mut *const PluginApi) -> Status;

impl ByteSpan {
    /// Create a borrowed span for an FFI call. The caller must ensure that the
    /// backing bytes outlive the call (or the plugin table, as appropriate).
    pub const fn from_static(bytes: &'static [u8]) -> Self {
        Self {
            data: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_layout_has_required_fields() {
        let host = HostApi {
            struct_size: core::mem::size_of::<HostApi>(),
            abi_version: ABI_VERSION,
            context: core::ptr::null_mut(),
            register_node_type: None,
        };
        assert_eq!(host.struct_size, core::mem::size_of_val(&host));
        assert_eq!(Status::Ok as i32, 0);
        assert_eq!(ENTRY_SYMBOL.last(), Some(&0));
    }

    #[test]
    fn static_span_matches_bytes() {
        let span = ByteSpan::from_static(b"example/source");
        assert_eq!(span.len, 14);
        assert!(!span.data.is_null());
    }
}
