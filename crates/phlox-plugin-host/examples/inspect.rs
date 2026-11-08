fn main() {
    let path = std::env::args_os()
        .nth(1)
        .expect("usage: inspect <trusted-plugin.so>");
    let plugin = unsafe { phlox_plugin_host::load(path) }.expect("plugin must load");
    println!("{} v{}", plugin.info.id, plugin.info.version);
    for node in &plugin.info.nodes {
        println!("  {}", node.id);
    }
}
