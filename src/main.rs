//! Dynamic (subprocess) entrypoint for the calibre-web plugin.
//!
//! The toolkit's `serve_service_plugin!` emits `fn main`, serving this plugin over the orca
//! socket. Dynamic replacement for the retired cdylib export — the plugin is a
//! `[[bin]]`, owns no runtime, and reaches orca only through the socket.
plugin_toolkit::serve_service_plugin! {
    name: "calibre-web",
    target_compat: "any",
    backend: calibre_web::CalibreWebBackend::new("calibre-web"),
}
