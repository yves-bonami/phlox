use phlox_plugin_sdk::{Registrar, Result};

fn register(registrar: &mut Registrar<'_>) -> Result<()> {
    registrar.node_type("example/source", r#"{"type":"object","properties":{"interval_ms":{"type":"integer","minimum":50,"maximum":60000,"default":500}},"x-phlox-role":"source","x-phlox-ports":{"outputs":[{"name":"out","value_type":"Integer"}]}}"#)?;
    registrar.node_type("example/sink", r#"{"type":"object","properties":{},"x-phlox-role":"sink","x-phlox-ports":{"inputs":[{"name":"in","value_type":"Integer"}]}}"#)?;
    Ok(())
}

phlox_plugin_sdk::export_plugin!(
    id: "phlox-example",
    version: "0.8.0",
    register: register,
);

// Separate optional execution ABI; no Rust allocations cross the library boundary.
use phlox_plugin_sdk::abi;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicU64, Ordering};

static STATE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn poll(node: abi::ByteSpan, host: *const abi::OutputApi) -> abi::Status {
    if host.is_null() || node.data.is_null() { return abi::Status::InvalidArgument; }
    let bytes = unsafe { std::slice::from_raw_parts(node.data, node.len) };
    if bytes != b"example/source" { return abi::Status::InvalidArgument; }
    let host = unsafe { &*host };
    let Some(emit) = host.emit else { return abi::Status::InvalidArgument };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut state = STATE.load(Ordering::Relaxed) ^ now;
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    STATE.store(state, Ordering::Relaxed);
    let json = format!("{}", state % 1000);
    unsafe { emit(host.context, abi::ByteSpan::from_static(b"out"), abi::ByteSpan { data: json.as_ptr(), len: json.len() }) }
}

unsafe extern "C" fn consume(node: abi::ByteSpan, json: abi::ByteSpan) -> abi::Status {
    if node.data.is_null() || json.data.is_null() { return abi::Status::InvalidArgument; }
    let node = unsafe { std::slice::from_raw_parts(node.data, node.len) };
    if node != b"example/sink" { return abi::Status::InvalidArgument; }
    let input = unsafe { std::slice::from_raw_parts(json.data, json.len) };
    let Ok(value) = std::str::from_utf8(input) else { return abi::Status::InvalidArgument };
    println!("[example/sink] received {value}");
    abi::Status::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn phlox_plugin_execution_v1() -> *const abi::ExecutionApi {
    struct Table(abi::ExecutionApi);
    unsafe impl Sync for Table {}
    static API: Table = Table(abi::ExecutionApi {
        struct_size: std::mem::size_of::<abi::ExecutionApi>(),
        poll: Some(poll),
        consume: Some(consume),
    });
    &API.0
}

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
static INSTANCES: LazyLock<Mutex<HashMap<u64, (bool, u64)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

unsafe extern "C" fn create_instance(node: abi::ByteSpan, instance: abi::ByteSpan, config: abi::ByteSpan, output: *mut u64) -> abi::Status {
    if node.data.is_null() || instance.data.is_null() || config.data.is_null() || output.is_null() {
        return abi::Status::InvalidArgument;
    }
    let kind = unsafe { std::slice::from_raw_parts(node.data, node.len) };
    if kind != b"example/source" && kind != b"example/sink" { return abi::Status::InvalidArgument; }
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    INSTANCES.lock().unwrap().insert(handle, (false, 0));
    unsafe { *output = handle; }
    println!("[example] create instance {handle}");
    abi::Status::Ok
}
unsafe extern "C" fn start_instance(handle: u64) -> abi::Status {
    let mut items = INSTANCES.lock().unwrap();
    let Some(started) = items.get_mut(&handle) else { return abi::Status::InvalidArgument };
    started.0 = true;
    println!("[example] start instance {handle}");
    abi::Status::Ok
}
unsafe extern "C" fn stop_instance(handle: u64) -> abi::Status {
    let mut items = INSTANCES.lock().unwrap();
    let Some(started) = items.get_mut(&handle) else { return abi::Status::InvalidArgument };
    started.0 = false;
    println!("[example] stop instance {handle}");
    abi::Status::Ok
}
unsafe extern "C" fn destroy_instance(handle: u64) -> abi::Status {
    if INSTANCES.lock().unwrap().remove(&handle).is_none() { return abi::Status::InvalidArgument; }
    println!("[example] destroy instance {handle}");
    abi::Status::Ok
}
#[unsafe(no_mangle)]
pub extern "C" fn phlox_plugin_lifecycle_v1() -> *const abi::LifecycleApi {
    struct Table(abi::LifecycleApi);
    unsafe impl Sync for Table {}
    static TABLE: Table = Table(abi::LifecycleApi {
        struct_size: std::mem::size_of::<abi::LifecycleApi>(),
        create: Some(create_instance), start: Some(start_instance),
        stop: Some(stop_instance), destroy: Some(destroy_instance),
    });
    &TABLE.0
}

unsafe extern "C" fn poll_instance(handle: u64, output: *const abi::OutputApi) -> abi::Status {
    if output.is_null() { return abi::Status::InvalidArgument; }
    let value = {
        let mut instances = INSTANCES.lock().unwrap();
        let Some(entry) = instances.get_mut(&handle) else { return abi::Status::InvalidArgument };
        if !entry.0 { return abi::Status::InvalidArgument; }
        entry.1 = entry.1.wrapping_add(1);
        // Separate reproducible test sequence for each node instance.
        (entry.1.wrapping_mul(73).wrapping_add(handle.wrapping_mul(131))) % 1000
    };
    let json = value.to_string();
    let output = unsafe { &*output };
    let Some(emit) = output.emit else { return abi::Status::InvalidArgument };
    unsafe { emit(output.context, abi::ByteSpan::from_static(b"out"), abi::ByteSpan { data: json.as_ptr(), len: json.len() }) }
}
unsafe extern "C" fn consume_instance(handle: u64, port: abi::ByteSpan, json: abi::ByteSpan) -> abi::Status {
    if port.data.is_null() || json.data.is_null() { return abi::Status::InvalidArgument; }
    let ready = INSTANCES.lock().unwrap().get(&handle).is_some_and(|v| v.0);
    if !ready || unsafe { std::slice::from_raw_parts(port.data, port.len) } != b"in" { return abi::Status::InvalidArgument; }
    let bytes = unsafe { std::slice::from_raw_parts(json.data, json.len) };
    let Ok(value) = std::str::from_utf8(bytes) else { return abi::Status::InvalidArgument };
    println!("[example/sink] instance {handle} received {value}");
    abi::Status::Ok
}
#[unsafe(no_mangle)]
pub extern "C" fn phlox_plugin_instance_execution_v1() -> *const abi::InstanceExecutionApi {
    struct Table(abi::InstanceExecutionApi);
    unsafe impl Sync for Table {}
    static TABLE: Table = Table(abi::InstanceExecutionApi {
        struct_size: std::mem::size_of::<abi::InstanceExecutionApi>(),
        poll: Some(poll_instance), consume: Some(consume_instance), process: None,
    });
    &TABLE.0
}
