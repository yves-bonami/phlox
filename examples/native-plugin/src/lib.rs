use phlox_plugin_sdk::{Registrar, Result};

fn register(registrar: &mut Registrar<'_>) -> Result<()> {
    registrar.node_type("example/source", r#"{"type":"object","properties":{}}"#)?;
    registrar.node_type("example/sink", r#"{"type":"object","properties":{}}"#)?;
    Ok(())
}

phlox_plugin_sdk::export_plugin!(
    id: "phlox-example",
    version: "0.1.0",
    register: register,
);
