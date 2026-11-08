use phlox_plugin_sdk::{Registrar, Result};

fn register(registrar: &mut Registrar<'_>) -> Result<()> {
    registrar.node_type("example/source", r#"{"type":"object","properties":{},"x-phlox-ports":{"outputs":[{"name":"out","value_type":"Integer"}]}}"#)?;
    registrar.node_type("example/sink", r#"{"type":"object","properties":{},"x-phlox-ports":{"inputs":[{"name":"in","value_type":"Integer"}]}}"#)?;
    Ok(())
}

phlox_plugin_sdk::export_plugin!(
    id: "phlox-example",
    version: "0.3.0",
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
