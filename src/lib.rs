#![allow(clippy::disallowed_types)]

use plugin_toolkit::service::{
    BoxFuture, Routes, Runtime, ServiceBackend, ServiceCapability, ServiceError, ServiceStatus,
    WorkloadSpec,
};

#[derive(Debug, Clone)]
pub struct CalibreWebBackend {
    provider: &'static str,
}

impl CalibreWebBackend {
    pub fn new(provider: &'static str) -> Self {
        Self { provider }
    }
}

impl ServiceBackend for CalibreWebBackend {
    fn provider(&self) -> &str {
        self.provider
    }
    fn runtimes(&self) -> Vec<Runtime> {
        vec![Runtime::Docker, Runtime::Podman, Runtime::Lxc, Runtime::Vm]
    }

    fn capabilities(&self) -> Vec<ServiceCapability> {
        vec![
            ServiceCapability::Deploy,
            ServiceCapability::Backup,
            ServiceCapability::Restore,
            ServiceCapability::Configure,
            ServiceCapability::Status,
        ]
    }

    fn default_port(&self) -> u16 {
        8083
    }
    fn data_paths(&self) -> Vec<String> {
        vec!["/config".to_string()]
    }

    fn workload_spec<'a>(
        &'a self,
        _runtime: Runtime,
        _instance: &'a str,
        _routes: &'a Routes,
    ) -> BoxFuture<'a, Result<WorkloadSpec, ServiceError>> {
        Box::pin(async move { Err(ServiceError::unimplemented("calibre-web.workload_spec")) })
    }

    fn configure<'a>(
        &'a self,
        _instance: &'a str,
        _routes: &'a Routes,
        _config: &'a str,
    ) -> BoxFuture<'a, Result<(), ServiceError>> {
        Box::pin(async move { Err(ServiceError::unimplemented("calibre-web.configure")) })
    }

    fn status<'a>(
        &'a self,
        _instance: &'a str,
        _routes: &'a Routes,
    ) -> BoxFuture<'a, Result<ServiceStatus, ServiceError>> {
        Box::pin(async move { Err(ServiceError::unimplemented("calibre-web.status")) })
    }
}

pub mod api;
mod cwa;
pub mod tools;

use plugin_toolkit::clap;
use plugin_toolkit::media::{
    Capability, MediaBackend, MediaError, MediaIdentity, MediaType, MediaUnit, MediaUrl, SeriesRef,
    Source, SourceKind, Variant,
};

#[plugin_toolkit::endpoint_resource(plugin = "calibre-web")]
pub struct CalibreWebEndpoint {
    pub name: String,
    pub username: String,
    #[secret]
    pub password: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CwMedia;

impl CwMedia {
    async fn connect(&self) -> Result<api::Cw, MediaError> {
        let rows = endpoint_db::list()
            .map_err(|e| MediaError::Transport(format!("read endpoints: {e}")))?;
        let ep = rows
            .into_iter()
            .filter(|r| r.enabled)
            .min_by(|a, b| a.name.cmp(&b.name))
            .ok_or_else(|| MediaError::NotFound("no enabled calibre-web endpoint".into()))?;
        tools::connect(&ep.name)
            .await
            .map_err(|e| MediaError::Transport(format!("{e:#}")))
    }
}

#[plugin_toolkit::orca_async]
impl MediaBackend for CwMedia {
    fn name(&self) -> &str {
        "calibre-web"
    }
    fn media_type(&self) -> MediaType {
        MediaType::Ebooks
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::ServedBy, Capability::Url, Capability::Units]
    }
    fn endpoint(&self) -> String {
        String::new()
    }

    async fn url(&self) -> Result<MediaUrl, MediaError> {
        Ok(MediaUrl {
            primary: self.connect().await?.base_url().to_string(),
            alternates: Vec::new(),
        })
    }

    async fn units(&self) -> Result<Vec<MediaUnit>, MediaError> {
        let cw = self.connect().await?;
        let books = cw
            .books()
            .await
            .map_err(|e| MediaError::Transport(format!("{e:#}")))?;
        Ok(books
            .iter()
            .filter_map(|b| to_unit(cw.base_url(), b))
            .collect())
    }
}

pub fn to_unit(base_url: &str, book: &api::Book) -> Option<MediaUnit> {
    if book.title.is_empty() {
        return None;
    }
    Some(MediaUnit {
        media_type: MediaType::Ebooks,
        identity: MediaIdentity {
            title: book.title.clone(),
            year: book
                .published
                .as_deref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok())
                .filter(|y| *y >= 1000),
            external_ids: Vec::new(),
            series: book.series.as_ref().map(|name| SeriesRef {
                name: name.clone(),
                sequence: book.series_index.clone(),
            }),
        },
        variants: book
            .formats
            .iter()
            .map(|f| Variant {
                quality: None,
                container: Some(f.to_ascii_lowercase()),
                tracks: Vec::new(),
                locations: Vec::new(),
            })
            .collect(),
        sources: vec![Source {
            method: SourceKind::AppStream,
            by: "calibre-web".to_string(),
            url: book
                .id
                .map(|id| format!("{}/book/{id}", base_url.trim_end_matches('/'))),
        }],
        downloaded_by: Vec::new(),
        served_by: vec!["calibre-web".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_toolkit::serde_json::{self, json};
    use std::sync::{Arc, Mutex};

    #[test]
    fn declares_provider() {
        let b = CalibreWebBackend::new("calibre-web");
        assert_eq!(b.provider(), "calibre-web");
    }

    #[test]
    fn media_facet_serves_ebook_units() {
        assert_eq!(CwMedia.name(), "calibre-web");
        assert_eq!(CwMedia.media_type(), MediaType::Ebooks);
        for cap in [Capability::ServedBy, Capability::Url, Capability::Units] {
            assert!(CwMedia.capabilities().contains(&cap), "missing {cap:?}");
        }
    }

    type Call = (String, String);

    fn serve_with<F>(route: impl Fn(&str) -> Option<(u16, String)> + 'static, body: F) -> Vec<Call>
    where
        F: FnOnce(),
    {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        plugin_toolkit::capsink::with_cap_sink(
            Box::new(move |cap: &str, req: &str| {
                assert_eq!(cap, "http.request");
                let req: serde_json::Value = serde_json::from_str(req).unwrap();
                let url = req["url"].as_str().unwrap().to_string();
                let method = req["method"].as_str().unwrap().to_string();
                let auth = req["headers"].as_array().unwrap().iter().any(|h| {
                    h[0].as_str().unwrap().eq_ignore_ascii_case("authorization")
                        && h[1] == "Basic YXV0b21hdGlvbjpwdw=="
                });
                assert!(auth, "missing basic auth on {url}");
                let path = url.trim_start_matches("http://cw.test").to_string();
                log.lock().unwrap().push((method, path.clone()));
                let (status, body) = route(&path)
                    .or_else(|| catalog_nav(&path))
                    .unwrap_or((404, "Not Found".to_string()));
                Ok(json!({"status": status, "headers": [], "body": body.into_bytes()}).to_string())
            }),
            body,
        );
        let out = seen.lock().unwrap().clone();
        out
    }

    fn nav_feed(entries: &[(&str, &str)]) -> String {
        let entries: String = entries
            .iter()
            .map(|(title, href)| {
                format!(
                    r#"<entry><title>{title}</title><id>{href}</id><link rel="subsection" type="application/atom+xml;profile=opds-catalog" href="{href}"/></entry>"#
                )
            })
            .collect();
        format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><link rel="self" href="/opds?"/><author><name>Calibre-Web</name><uri>https://github.com/crocodilestick/calibre-web-automated</uri></author>{entries}</feed>"#
        )
    }

    fn catalog_nav(path: &str) -> Option<(u16, String)> {
        let body = match path {
            "/opds" => nav_feed(&[
                ("Alphabetical Books", "/opds/books"),
                ("Recently added Books", "/opds/new"),
                ("Authors", "/opds/author"),
                ("Series", "/opds/series"),
            ]),
            "/opds/books" | "/opds/series" | "/opds/author" => nav_feed(&[
                ("All", &format!("{path}/letter/00")),
                ("A", &format!("{path}/letter/A")),
            ]),
            _ => return None,
        };
        Some((200, body))
    }

    fn block<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn cw() -> api::Cw {
        api::Cw::new("http://cw.test/", "automation", "pw").unwrap()
    }

    fn feed(entries: &str, next: Option<usize>) -> String {
        let next = next
            .map(|n| format!(r#"<link rel="next" href="/opds/books/letter/00?offset={n}"/>"#))
            .unwrap_or_default();
        format!(r#"<feed xmlns="http://www.w3.org/2005/Atom">{next}{entries}</feed>"#)
    }

    fn book(id: u64, title: &str) -> String {
        format!(
            r#"<entry><title>{title}</title><id>urn:uuid:u{id}</id><author><name>A</name></author>
            <published>0101-01-01T00:00:00+00:00</published>
            <content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml">SERIES: S [{id}]<br/></div></content>
            <link rel="http://opds-spec.org/acquisition" href="/opds/download/{id}/epub/" title="EPUB"/></entry>"#
        )
    }

    #[test]
    fn books_follow_next_offsets_over_delegated_http() {
        let mut got = Vec::new();
        let calls = serve_with(
            |path| match path {
                "/opds/books/letter/00" => Some((200, feed(&book(1, "One"), Some(1)))),
                "/opds/books/letter/00?offset=1" => Some((200, feed(&book(2, "Two"), None))),
                _ => None,
            },
            || got = block(cw().books()).unwrap(),
        );
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].series_index.as_deref(), Some("2"));
        let u = to_unit("http://cw.test/", &got[0]).unwrap();
        assert_eq!(u.identity.year, None);
        assert_eq!(u.identity.series.as_ref().unwrap().name, "S");
        assert_eq!(u.variants[0].container.as_deref(), Some("epub"));
        assert_eq!(u.sources[0].url.as_deref(), Some("http://cw.test/book/1"));
    }

    #[test]
    fn pages_stop_at_the_limit_and_resume_mid_page() {
        let two = format!("{}{}", book(1, "One"), book(2, "Two"));
        let mut first = api::Page::default();
        let mut rest = api::Page::default();
        let calls = serve_with(
            move |path| match path {
                "/opds/books/letter/00" => Some((200, feed(&two, Some(2)))),
                "/opds/books/letter/00?offset=1" => Some((200, feed(&book(2, "Two"), Some(2)))),
                "/opds/books/letter/00?offset=2" => Some((200, feed(&book(3, "Three"), None))),
                _ => None,
            },
            || {
                first = block(cw().books_page(0, 1)).unwrap();
                rest = block(cw().books_page(1, 10)).unwrap();
            },
        );
        assert_eq!(first.books.len(), 1);
        assert_eq!(first.next_offset, Some(1));
        assert_eq!(rest.books.len(), 2);
        assert_eq!(rest.next_offset, None);
        assert_eq!(calls.len(), 7, "{calls:?}");
    }

    #[test]
    fn a_limit_on_a_page_boundary_reports_the_next_page() {
        let mut got = api::Page::default();
        serve_with(
            |path| match path {
                "/opds/books/letter/00" => Some((200, feed(&book(1, "One"), Some(1)))),
                _ => None,
            },
            || got = block(cw().books_page(0, 1)).unwrap(),
        );
        assert_eq!(got.next_offset, Some(1));
    }

    #[test]
    fn a_next_offset_that_skips_or_repeats_fails() {
        for next in [0, 5] {
            serve_with(
                move |path| {
                    path.contains("/letter/")
                        .then(|| (200, feed(&book(1, "One"), Some(next))))
                },
                || {
                    let e = block(cw().books()).unwrap_err().to_string();
                    assert!(e.contains("does not follow"), "{e}");
                },
            );
        }
    }

    #[test]
    fn an_endless_growing_offset_hits_the_page_cap() {
        let calls = serve_with(
            |path| {
                if !path.contains("/letter/") {
                    return None;
                }
                let off: usize = path
                    .split_once("offset=")
                    .map_or(0, |(_, o)| o.parse().unwrap());
                Some((200, feed(&book(off as u64, "B"), Some(off + 1))))
            },
            || {
                let e = block(cw().books()).unwrap_err().to_string();
                assert!(e.contains("pages"), "{e}");
            },
        );
        assert_eq!(calls.len(), 1002);
    }

    #[test]
    fn a_non_2xx_mid_pagination_fails_the_read() {
        serve_with(
            |path| match path {
                "/opds/books/letter/00" => Some((200, feed(&book(1, "One"), Some(1)))),
                "/opds/books/letter/00?offset=1" => Some((502, "Bad Gateway".to_string())),
                _ => None,
            },
            || {
                let e = block(cw().books()).unwrap_err().to_string();
                assert!(e.contains("HTTP 502"), "{e}");
            },
        );
    }

    #[test]
    fn authorization_never_appears_in_errors() {
        serve_with(
            |_| Some((401, "Authorization: Basic YXV0b21hdGlvbjpwdw==".to_string())),
            || {
                let e = format!("{:#}", block(cw().books()).unwrap_err());
                assert!(!e.contains("YXV0b21hdGlvbjpwdw=="), "{e}");
                let e = format!("{:#}", block(cw().reconnect()).unwrap_err());
                assert!(!e.contains("YXV0b21hdGlvbjpwdw=="), "{e}");
            },
        );
    }

    #[test]
    fn units_without_an_endpoint_is_not_found() {
        let e = plugin_toolkit::capsink::with_cap_sink(
            Box::new(|cap: &str, _: &str| match cap {
                "db.op" => Ok(json!({"rows": []}).to_string()),
                _ => Err(format!("unexpected {cap}")),
            }),
            || block(CwMedia.units()).unwrap_err(),
        );
        assert!(matches!(e, MediaError::NotFound(_)), "{e:?}");
    }

    #[test]
    fn series_and_authors_read_catalog_entries() {
        let entry = |p: &str| {
            feed(
                &format!(r#"<entry><title>Name</title><id>/opds/{p}/9</id></entry>"#),
                None,
            )
        };
        let calls = serve_with(
            move |path| match path {
                "/opds/series/letter/00" => Some((200, entry("series"))),
                "/opds/author/letter/00" => Some((200, entry("author"))),
                _ => None,
            },
            || {
                assert_eq!(block(cw().series()).unwrap()[0].id, 9);
                assert_eq!(block(cw().authors()).unwrap()[0].name, "Name");
            },
        );
        assert_eq!(calls.len(), 6);
    }

    #[test]
    fn off_origin_root_links_are_never_fetched() {
        for href in [
            "@evil.test/opds/books",
            ".evil.test/opds/books",
            "//evil.test/opds/books",
            "http://evil.test/opds/books",
            "https://cw.test/opds/books",
            "http://cw.test.evil.test/opds/books",
            "http://cw.test@evil.test/opds/books",
        ] {
            let root = nav_feed(&[("Alphabetical Books", href)]);
            let calls = serve_with(
                move |path| (path == "/opds").then(|| (200, root.clone())),
                || {
                    let e = block(cw().books()).unwrap_err().to_string();
                    assert!(e.contains("does not serve"), "{href}: {e}");
                },
            );
            assert_eq!(
                calls,
                vec![("GET".to_string(), "/opds".to_string())],
                "{href}"
            );
        }
    }

    #[test]
    fn an_off_origin_all_entry_fails_without_fetching() {
        let calls = serve_with(
            |path| {
                (path == "/opds/books").then(|| {
                    (
                        200,
                        nav_feed(&[("All", "http://evil.test/opds/books/letter/00")]),
                    )
                })
            },
            || {
                let e = block(cw().books()).unwrap_err().to_string();
                assert!(e.contains("refusing link"), "{e}");
            },
        );
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|(_, p)| p.starts_with('/')));
    }

    #[test]
    fn same_origin_absolute_links_resolve_to_paths() {
        let calls = serve_with(
            |path| match path {
                "/opds" => Some((
                    200,
                    nav_feed(&[("Alphabetical Books", "http://cw.test/opds/books?x=1")]),
                )),
                "/opds/books/letter/00" => Some((200, feed(&book(1, "One"), None))),
                _ => None,
            },
            || assert_eq!(block(cw().books()).unwrap().len(), 1),
        );
        assert_eq!(calls[1].1, "/opds/books");
    }

    #[test]
    fn books_survive_an_unavailable_series_listing() {
        let calls = serve_with(
            |path| {
                (path == "/opds/books/letter/00").then(|| {
                    (
                        200,
                        feed(
                            r#"<entry><title>T</title><id>urn:uuid:u1</id></entry>"#,
                            None,
                        ),
                    )
                })
            },
            || {
                let books = block(cw().books()).unwrap();
                assert_eq!(books.len(), 1);
                assert_eq!(books[0].series, None);
            },
        );
        assert_eq!(calls.last().unwrap().1, "/login");
    }

    #[test]
    fn a_calibre_content_server_root_is_not_a_catalog() {
        serve_with(
            |path| {
                match path {
                "/opds" => Some((
                    200,
                    r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>calibre Library</title><entry><title>By Newest</title><id>calibre:navcatalog:4f</id><link href="/opds/navcatalog/4f?library_id=Calibre_Library"/></entry></feed>"#.to_string(),
                )),
                _ => None,
            }
            },
            || {
                let e = block(cw().books()).unwrap_err().to_string();
                assert!(
                    e.contains("does not serve a Calibre-Web OPDS catalog"),
                    "{e}"
                );
            },
        );
    }

    #[test]
    fn reconnect_reports_a_disabled_route() {
        serve_with(
            |_| None,
            || {
                let e = block(cw().reconnect()).unwrap_err().to_string();
                assert!(e.contains("CALIBRE_RECONNECT"), "{e}");
            },
        );
        let calls = serve_with(
            |p| (p == "/reconnect").then(|| (200, "{}".to_string())),
            || block(cw().reconnect()).unwrap(),
        );
        assert_eq!(calls, vec![("GET".to_string(), "/reconnect".to_string())]);
    }

    #[test]
    fn error_bodies_are_truncated_and_scrubbed() {
        serve_with(
            |_| {
                Some((
                    500,
                    format!(
                        r#"{{"password":"supersecretvalue123","pad":"{}"}}"#,
                        "x".repeat(2000)
                    ),
                ))
            },
            || {
                let e = block(cw().books()).unwrap_err().to_string();
                assert!(e.len() < 400, "{}", e.len());
                assert!(!e.contains("supersecretvalue123"), "{e}");
            },
        );
    }

    #[test]
    fn empty_username_is_refused() {
        assert!(api::Cw::new("http://cw.test", " ", "pw").is_err());
    }

    #[test]
    fn connect_fails_when_the_password_secret_is_missing() {
        let caps = Arc::new(Mutex::new(Vec::new()));
        let log = caps.clone();
        let e = plugin_toolkit::capsink::with_cap_sink(
            Box::new(move |cap: &str, _: &str| {
                log.lock().unwrap().push(cap.to_string());
                match cap {
                    "secret.op" => Ok(json!({"value": null}).to_string()),
                    _ => Err(format!("unexpected {cap}")),
                }
            }),
            || block(tools::connect("cw")).err().unwrap(),
        );
        assert!(format!("{e:#}").contains("calibre-web"), "{e:#}");
        assert_eq!(*caps.lock().unwrap(), vec!["secret.op"]);
    }
}
