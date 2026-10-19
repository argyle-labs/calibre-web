plugin_toolkit::instrument::bootstrap!();

#[allow(unused_imports)]
use calibre_web::tools as _;

fn main() -> plugin_toolkit::anyhow::Result<()> {
    plugin_toolkit::plugin::Plugin::named("calibre-web")
        .version(env!("CARGO_PKG_VERSION"))
        .tools(["calibre-web."])
        .service(calibre_web::CalibreWebBackend::new("calibre-web"))
        .media(calibre_web::CwMedia)
        .serve()
}
