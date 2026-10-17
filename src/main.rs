plugin_toolkit::instrument::bootstrap!();

fn main() -> plugin_toolkit::anyhow::Result<()> {
    plugin_toolkit::plugin::Plugin::named("calibre-web")
        .version(env!("CARGO_PKG_VERSION"))
        .service(calibre_web::CalibreWebBackend::new("calibre-web"))
        .serve()
}
