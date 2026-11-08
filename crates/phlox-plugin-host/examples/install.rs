use phlox_plugin_host::registry::PluginRegistry;
use std::path::Path;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .expect("usage: install <plugin-root> <package-dir>");
    let source = args
        .next()
        .expect("usage: install <plugin-root> <package-dir>");
    let mut registry = PluginRegistry::new(root);
    println!(
        "Installed at {}",
        registry.install(Path::new(&source)).unwrap().display()
    );
    for (path, error) in registry.refresh() {
        eprintln!("{}: {error:?}", path.display());
    }
    for info in registry.active() {
        println!("Activated {} v{}", info.id, info.version);
    }
}
