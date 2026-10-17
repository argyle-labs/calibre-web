use plugin_toolkit::prelude::*;

use crate::api::{Book, CatalogEntry, Cw};
use crate::endpoint_db;

pub async fn connect(name: &str) -> Result<Cw> {
    let password = plugin_toolkit::secrets::resolve_scoped("calibre-web", name, "password")?;
    let row = endpoint_db::require(name)?;
    let base_url = route::resolve_reachable(name, &row.routes, false).await?;
    Cw::new(&base_url, &row.username, password.expose_secret())
}

#[orca_struct(args)]
pub struct EndpointArgs {
    /// Registered calibre-web endpoint name.
    #[arg(long)]
    pub name: String,
}

/// List every book with title, authors, series + index, formats, languages and
/// publication date, read from the OPDS catalog (`/opds/books/letter/00`).
/// This is the owned-ebook inventory for ebook and audiobook parity.
#[orca_tool(domain = "calibre-web", verb = "books.list", role = "read")]
async fn calibre_web_books_list(args: EndpointArgs, _ctx: &ToolCtx) -> Result<Vec<Book>> {
    connect(&args.name).await?.books().await
}

/// List series from the OPDS catalog (`/opds/series/letter/00`).
#[orca_tool(domain = "calibre-web", verb = "series.list", role = "read")]
async fn calibre_web_series_list(args: EndpointArgs, _ctx: &ToolCtx) -> Result<Vec<CatalogEntry>> {
    connect(&args.name).await?.series().await
}

/// List authors from the OPDS catalog (`/opds/author/letter/00`).
#[orca_tool(domain = "calibre-web", verb = "authors.list", role = "read")]
async fn calibre_web_authors_list(args: EndpointArgs, _ctx: &ToolCtx) -> Result<Vec<CatalogEntry>> {
    connect(&args.name).await?.authors().await
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshReport {
    pub refreshed: bool,
}

/// Reload the Calibre library after metadata.db changed underneath
/// calibre-web (`GET /reconnect`). Needs calibre-web started with `-r` or
/// `CALIBRE_RECONNECT=1`, which opens an unauthenticated, CSRF-free route:
/// enable it only behind the LAN/Caddy boundary, or block `/reconnect` at the
/// proxy except for orca.
#[orca_tool(
    domain = "calibre-web",
    verb = "library.refresh",
    role = "admin",
    execute_gated = true
)]
async fn calibre_web_library_refresh(args: EndpointArgs, _ctx: &ToolCtx) -> Result<RefreshReport> {
    connect(&args.name).await?.reconnect().await?;
    Ok(RefreshReport { refreshed: true })
}

#[cfg(test)]
mod tests {
    #[test]
    fn tools_declare_roles_and_gating() {
        let role = |n| plugin_toolkit::dispatch::required_role(n);
        for n in [
            "calibre-web.books.list",
            "calibre-web.series.list",
            "calibre-web.authors.list",
        ] {
            assert_eq!(role(n), Some("read"), "{n}");
        }
        assert_eq!(role("calibre-web.library.refresh"), Some("admin"));
        let gated = plugin_toolkit::dispatch::execute_gated_names();
        assert!(gated.contains(&"calibre-web.library.refresh"));
        assert!(!gated.contains(&"calibre-web.books.list"));
    }
}
