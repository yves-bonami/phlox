use phlox_plugin_sdk::{Registrar, Result};

fn register(registrar: &mut Registrar<'_>) -> Result<()> {
    registrar.node_type("example/source", r#"{"type":"object","properties":{},"x-phlox-ports":{"outputs":[{"name":"out","value_type":"String"}]}}"#)?;
    registrar.node_type("example/sink", r#"{"type":"object","properties":{},"x-phlox-ports":{"inputs":[{"name":"in","value_type":"String"}]}}"#)?;
    Ok(())
}

phlox_plugin_sdk::export_plugin!(
    id: "phlox-example",
    version: "0.2.0",
    register: register,
);
