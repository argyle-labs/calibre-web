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

#[orca_struct(args)]
pub struct ListArgs {
    /// Registered calibre-web endpoint name.
    #[arg(long)]
    pub name: String,
    /// Catalog offset to start at; pass the previous page's `next_offset`.
    #[arg(long)]
    pub offset: Option<usize>,
    /// Entries per page. Default 500, max 2000.
    #[arg(long)]
    pub limit: Option<usize>,
}

pub const LIST_DEFAULT: usize = 500;
pub const LIST_MAX: usize = 2000;

pub fn list_limit(limit: Option<usize>) -> Result<usize> {
    match limit.unwrap_or(LIST_DEFAULT) {
        0 => bail!("limit must be at least 1"),
        n if n > LIST_MAX => bail!("limit {n} exceeds the maximum of {LIST_MAX}"),
        n => Ok(n),
    }
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct BookPage {
    pub books: Vec<Book>,
    pub next_offset: Option<usize>,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPage {
    pub entries: Vec<CatalogEntry>,
    pub next_offset: Option<usize>,
}

/// List one page of books with title, authors, series + index, formats,
/// languages and publication date, read from the OPDS catalog
/// (`/opds/books/letter/00`). Follow `next_offset` until it is null.
/// This is the owned-ebook inventory for ebook and audiobook parity.
#[orca_tool(domain = "calibre-web", verb = "books.list", role = "read")]
async fn calibre_web_books_list(args: ListArgs, _ctx: &ToolCtx) -> Result<BookPage> {
    let limit = list_limit(args.limit)?;
    let page = connect(&args.name)
        .await?
        .books_page(args.offset.unwrap_or(0), limit)
        .await?;
    Ok(BookPage {
        books: page.books,
        next_offset: page.next_offset,
    })
}

/// List one page of series from the OPDS catalog (`/opds/series/letter/00`).
/// Follow `next_offset` until it is null.
#[orca_tool(domain = "calibre-web", verb = "series.list", role = "read")]
async fn calibre_web_series_list(args: ListArgs, _ctx: &ToolCtx) -> Result<CatalogPage> {
    let limit = list_limit(args.limit)?;
    let page = connect(&args.name)
        .await?
        .series_page(args.offset.unwrap_or(0), limit)
        .await?;
    Ok(CatalogPage {
        entries: page.entries,
        next_offset: page.next_offset,
    })
}

/// List one page of authors from the OPDS catalog (`/opds/author/letter/00`).
/// Follow `next_offset` until it is null.
#[orca_tool(domain = "calibre-web", verb = "authors.list", role = "read")]
async fn calibre_web_authors_list(args: ListArgs, _ctx: &ToolCtx) -> Result<CatalogPage> {
    let limit = list_limit(args.limit)?;
    let page = connect(&args.name)
        .await?
        .authors_page(args.offset.unwrap_or(0), limit)
        .await?;
    Ok(CatalogPage {
        entries: page.entries,
        next_offset: page.next_offset,
    })
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

    #[test]
    fn list_limit_defaults_and_caps() {
        use super::*;
        assert_eq!(list_limit(None).unwrap(), LIST_DEFAULT);
        assert_eq!(list_limit(Some(LIST_MAX)).unwrap(), LIST_MAX);
        assert!(list_limit(Some(LIST_MAX + 1)).is_err());
        assert!(list_limit(Some(0)).is_err());
    }
}
