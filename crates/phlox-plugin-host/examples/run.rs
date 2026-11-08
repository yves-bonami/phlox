//! Minimal native plugin execution proof: source -> host -> sink.
//! Usage: cargo run -p phlox-plugin-host --example run -- <trusted-plugin.so>
fn main() {
    let library = std::env::args_os()
        .nth(1)
        .expect("usage: run <trusted-plugin.so>");
    let plugin = unsafe { phlox_plugin_host::load(library) }.expect("load plugin");
    for _ in 0..5 {
        for (port, value) in plugin.poll("example/source").expect("poll source") {
            assert_eq!(port, "out");
            println!("[phlox] source.{port} = {value}");
            plugin
                .consume("example/sink", &value)
                .expect("send to sink");
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
