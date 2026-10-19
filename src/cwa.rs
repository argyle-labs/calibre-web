use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use plugin_toolkit::prelude::*;
use plugin_toolkit::progenitor_client::QueryParam;
use plugin_toolkit::reqwest::header::{HeaderMap, HeaderValue};

use crate::api::{error_excerpt, Book, Cw};

const LISTBOOKS_LIMIT: usize = 500;
const MAX_PAGES: usize = 1000;
const MAX_ROWS: usize = 200_000;

#[derive(plugin_toolkit::serde::Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
struct ListBooks {
    #[serde(default)]
    total: usize,
    #[serde(default)]
    rows: Vec<Row>,
}

#[derive(plugin_toolkit::serde::Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
struct Row {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    series: String,
    #[serde(default)]
    series_index: Option<Index>,
}

#[derive(plugin_toolkit::serde::Deserialize)]
#[serde(crate = "plugin_toolkit::serde", untagged)]
enum Index {
    Number(f64),
    Text(String),
}

impl Index {
    fn render(&self) -> Option<String> {
        match self {
            Index::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(format!("{n:.0}")),
            Index::Number(n) if n.is_finite() => Some(n.to_string()),
            Index::Number(_) => None,
            Index::Text(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        }
    }
}

#[derive(Default)]
struct Jar(Vec<(String, String)>);

impl Jar {
    fn absorb(&mut self, headers: &HeaderMap) -> usize {
        let mut set = 0;
        for (k, v) in headers.iter() {
            if !k.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            let Some((name, value)) = v
                .to_str()
                .unwrap_or_default()
                .split(';')
                .next()
                .and_then(|c| c.split_once('='))
            else {
                continue;
            };
            let (name, value) = (name.trim().to_string(), value.trim().to_string());
            self.0.retain(|(n, _)| *n != name);
            self.0.push((name, value));
            set += 1;
        }
        set
    }

    fn header(&self) -> Option<String> {
        (!self.0.is_empty()).then(|| {
            self.0
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}

fn sessions() -> &'static Mutex<HashMap<String, String>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    SESSIONS.get_or_init(Default::default)
}

fn csrf_token(html: &str) -> Option<String> {
    let at = html.find(r#"name="csrf_token""#)?;
    let tag_start = html[..at].rfind('<')?;
    let tag = &html[tag_start..at + html[at..].find('>')?];
    let v = &tag[tag.find(r#"value=""#)? + 7..];
    Some(v[..v.find('"')?].to_string())
}

enum Listing {
    Rows(Vec<Row>),
    NeedsLogin,
}

impl Cw {
    fn session_key(&self) -> String {
        format!("{}\n{}", self.base_url(), self.username())
    }

    pub async fn enrich_series(&self, books: &mut [Book]) -> Result<()> {
        if books.iter().all(|b| b.series.is_some()) {
            return Ok(());
        }
        let rows = self.list_books().await?;
        let by_uuid: HashMap<&str, &Row> = rows
            .iter()
            .filter(|r| !r.uuid.is_empty())
            .map(|r| (r.uuid.as_str(), r))
            .collect();
        let by_id: HashMap<u64, &Row> = rows.iter().filter_map(|r| Some((r.id?, r))).collect();
        for b in books.iter_mut().filter(|b| b.series.is_none()) {
            let Some(row) = by_uuid.get(b.uuid.as_str()).or_else(|| by_id.get(&b.id?)) else {
                continue;
            };
            let name = row.series.trim();
            if name.is_empty() {
                continue;
            }
            b.series = Some(name.to_string());
            b.series_index = row.series_index.as_ref().and_then(Index::render);
        }
        Ok(())
    }

    async fn list_books(&self) -> Result<Vec<Row>> {
        let cached = sessions()
            .lock()
            .map_err(|_| anyhow!("session cache poisoned"))?
            .get(&self.session_key())
            .cloned();
        if let Some(cookie) = cached {
            if let Listing::Rows(rows) = self.list_books_with(&cookie).await? {
                return Ok(rows);
            }
        }
        let cookie = self.login().await?;
        match self.list_books_with(&cookie).await? {
            Listing::Rows(rows) => Ok(rows),
            Listing::NeedsLogin => {
                self.forget_session();
                bail!("GET /ajax/listbooks: the new calibre-web session was not accepted")
            }
        }
    }

    fn forget_session(&self) {
        if let Ok(mut s) = sessions().lock() {
            s.remove(&self.session_key());
        }
    }

    async fn list_books_with(&self, cookie: &str) -> Result<Listing> {
        let mut out = Vec::new();
        for _ in 0..MAX_PAGES {
            let query = [
                QueryParam::new("offset", &out.len().to_string()),
                QueryParam::new("limit", &LISTBOOKS_LIMIT.to_string()),
                QueryParam::new("sort", &"id"),
                QueryParam::new("order", &"asc"),
            ];
            let req = self
                .http()
                .get(format!("{}/ajax/listbooks", self.base_url()))
                .query(&query)
                .header("cookie", cookie_value(cookie)?);
            let (status, _, body) = send("GET /ajax/listbooks", req).await?;
            if matches!(status, 300..=399 | 401 | 403) {
                return Ok(Listing::NeedsLogin);
            }
            if !(200..300).contains(&status) {
                bail!(
                    "GET /ajax/listbooks: HTTP {status} {}",
                    error_excerpt(&body)
                );
            }
            let page: ListBooks = match plugin_toolkit::serde_json::from_str(&body) {
                Ok(p) => p,
                Err(_) if body.trim_start().starts_with('<') => return Ok(Listing::NeedsLogin),
                Err(e) => bail!("GET /ajax/listbooks: expected JSON: {e}"),
            };
            let n = page.rows.len();
            out.extend(page.rows);
            if out.len() > MAX_ROWS {
                bail!("GET /ajax/listbooks: more than {MAX_ROWS} rows");
            }
            if n == 0 || out.len() >= page.total {
                return Ok(Listing::Rows(out));
            }
        }
        bail!("GET /ajax/listbooks: more than {MAX_PAGES} pages")
    }

    async fn login(&self) -> Result<String> {
        let req = self.http().get(format!("{}/login", self.base_url()));
        let (status, headers, body) = send("GET /login", req).await?;
        if !(200..300).contains(&status) {
            bail!("GET /login: HTTP {status}");
        }
        let mut jar = Jar::default();
        jar.absorb(&headers);
        let csrf = csrf_token(&body).unwrap_or_default();
        let form = [
            QueryParam::new("next", &"/"),
            QueryParam::new("csrf_token", &csrf),
            QueryParam::new("username", &self.username()),
            QueryParam::new("password", &self.password()),
            QueryParam::new("remember_me", &"on"),
            QueryParam::new("submit", &""),
        ];
        let mut req = self
            .http()
            .post(format!("{}/login", self.base_url()))
            .follow_redirects(false)
            .form_urlencoded(&form);
        if let Some(c) = jar.header() {
            req = req.header("cookie", cookie_value(&c)?);
        }
        let (status, headers, _) = send("POST /login", req).await?;
        if !(300..400).contains(&status) || jar.absorb(&headers) == 0 {
            bail!(
                "POST /login: HTTP {status}; calibre-web did not start a session for '{}'",
                self.username()
            );
        }
        let cookie = jar
            .header()
            .ok_or_else(|| anyhow!("POST /login: no session cookie"))?;
        sessions()
            .lock()
            .map_err(|_| anyhow!("session cache poisoned"))?
            .insert(self.session_key(), cookie.clone());
        Ok(cookie)
    }
}

fn cookie_value(cookie: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(cookie).map_err(|_| anyhow!("calibre-web set an invalid session cookie"))
}

async fn send(
    what: &str,
    req: plugin_toolkit::reqwest::RequestBuilder,
) -> Result<(u16, HeaderMap, String)> {
    let resp = req
        .send()
        .await
        .map_err(|e| anyhow!("{what}: {}", error_excerpt(&e.to_string())))?;
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let body = resp
        .text()
        .await
        .map_err(|e| anyhow!("{what}: {}", error_excerpt(&e.to_string())))?;
    Ok((status, headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_toolkit::serde_json::{self, json};
    use std::sync::Arc;

    type Reply = (u16, Vec<(String, String)>, String);
    type Seen = (String, String, Option<String>, String);

    fn serve(
        route: impl Fn(&str, &str, Option<&str>, &str) -> Reply + 'static,
        body: impl FnOnce(),
    ) -> Vec<Seen> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        plugin_toolkit::capsink::with_cap_sink(
            Box::new(move |_cap: &str, req: &str| {
                let req: serde_json::Value = serde_json::from_str(req).unwrap();
                let url = req["url"].as_str().unwrap();
                let path = url[url.find("://").unwrap() + 3..]
                    .split_once('/')
                    .map_or("/".to_string(), |(_, p)| format!("/{p}"));
                let method = req["method"].as_str().unwrap().to_string();
                let cookie = req["headers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|h| h[0].as_str().unwrap().eq_ignore_ascii_case("cookie"))
                    .map(|h| h[1].as_str().unwrap().to_string());
                let bytes: Vec<u8> = req["body"]
                    .as_array()
                    .map(|b| b.iter().map(|x| x.as_u64().unwrap() as u8).collect())
                    .unwrap_or_default();
                let form = String::from_utf8(bytes).unwrap();
                if method == "POST" && path == "/login" {
                    assert_eq!(req["follow_redirects"], json!(false));
                }
                let (status, headers, body) = route(&method, &path, cookie.as_deref(), &form);
                log.lock().unwrap().push((method, path, cookie, form));
                Ok(
                    json!({"status": status, "headers": headers, "body": body.into_bytes()})
                        .to_string(),
                )
            }),
            body,
        );
        let out = seen.lock().unwrap().clone();
        out
    }

    fn block<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn cookie(v: &str) -> Vec<(String, String)> {
        vec![("set-cookie".into(), format!("{v}; HttpOnly; Path=/"))]
    }

    const LOGIN_PAGE: &str = r#"<form><input type="hidden" name="next" value="/"><input type="hidden" name="csrf_token" value="tok-1"></form>"#;

    fn book(id: u64, uuid: &str, series: Option<&str>) -> Book {
        Book {
            id: Some(id),
            uuid: uuid.into(),
            title: format!("T{id}"),
            authors: vec![],
            series: series.map(String::from),
            series_index: None,
            formats: vec![],
            languages: vec![],
            published: None,
        }
    }

    fn rows(offset: usize, total: usize, per: usize) -> String {
        let rows: Vec<_> = (offset..(offset + per).min(total))
            .map(|i| json!({"id": i, "uuid": format!("u{i}"), "series": format!("S{i}"), "series_index": i as f64 + 0.5}))
            .collect();
        json!({"total": total, "totalNotFiltered": total, "rows": rows}).to_string()
    }

    fn cwa(page: usize) -> impl Fn(&str, &str, Option<&str>, &str) -> Reply + 'static {
        move |method, path, cookie, form| match (method, path.split('?').next().unwrap()) {
            ("GET", "/login") => (200, cookie_hdr("session=anon"), LOGIN_PAGE.into()),
            ("POST", "/login") => {
                assert_eq!(cookie, Some("session=anon"));
                assert!(form.contains("csrf_token=tok-1"), "{form}");
                assert!(form.contains("username=automation"), "{form}");
                assert!(form.contains("password=p%40ss%26w%3Drd"), "{form}");
                (302, cookie_hdr("session=authed"), String::new())
            }
            ("GET", "/ajax/listbooks") if cookie == Some("session=authed") => {
                let off: usize = path
                    .split("offset=")
                    .nth(1)
                    .and_then(|o| o.split('&').next())
                    .unwrap()
                    .parse()
                    .unwrap();
                (200, vec![], rows(off, 3, page))
            }
            ("GET", "/ajax/listbooks") => (302, vec![], String::new()),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn cookie_hdr(v: &str) -> Vec<(String, String)> {
        cookie(v)
    }

    fn cw(host: &str) -> Cw {
        Cw::new(&format!("http://{host}"), "automation", "p@ss&w=rd").unwrap()
    }

    #[test]
    fn series_merge_from_paged_listbooks_after_login() {
        let mut books = vec![
            book(0, "u0", None),
            book(9, "u2", None),
            book(7, "nope", Some("Kept")),
        ];
        let calls = serve(cwa(2), || {
            block(cw("merge.test").enrich_series(&mut books)).unwrap()
        });
        assert_eq!(books[0].series.as_deref(), Some("S0"));
        assert_eq!(books[0].series_index.as_deref(), Some("0.5"));
        assert_eq!(books[1].series.as_deref(), Some("S2"));
        assert_eq!(books[2].series.as_deref(), Some("Kept"));
        let paths: Vec<_> = calls.iter().map(|c| c.1.as_str()).collect();
        assert_eq!(paths[..2], ["/login", "/login"]);
        assert_eq!(paths.len(), 4);
        assert!(paths[2].contains("offset=0") && paths[3].contains("offset=2"));
    }

    #[test]
    fn the_session_is_reused_across_calls() {
        let calls = serve(cwa(10), || {
            let cw = cw("reuse.test");
            for _ in 0..3 {
                let mut books = vec![book(1, "u1", None)];
                block(cw.enrich_series(&mut books)).unwrap();
                assert_eq!(books[0].series.as_deref(), Some("S1"));
            }
        });
        assert_eq!(calls.iter().filter(|c| c.1 == "/login").count(), 2);
    }

    #[test]
    fn books_that_all_have_series_skip_the_login() {
        let calls = serve(cwa(10), || {
            let mut books = vec![book(1, "u1", Some("S"))];
            block(cw("skip.test").enrich_series(&mut books)).unwrap();
        });
        assert!(calls.is_empty());
    }

    #[test]
    fn an_expired_session_logs_in_again() {
        sessions().lock().unwrap().insert(
            "http://expired.test\nautomation".into(),
            "session=old".into(),
        );
        let calls = serve(cwa(10), || {
            let mut books = vec![book(1, "u1", None)];
            block(cw("expired.test").enrich_series(&mut books)).unwrap();
            assert_eq!(books[0].series.as_deref(), Some("S1"));
        });
        assert_eq!(calls[0].2.as_deref(), Some("session=old"));
        assert_eq!(calls.iter().filter(|c| c.1 == "/login").count(), 2);
    }

    #[test]
    fn a_rejected_login_never_echoes_the_password_or_cookie() {
        serve(
            |method, _, _, _| match method {
                "GET" => (200, cookie("session=anon-secret"), LOGIN_PAGE.into()),
                _ => (
                    200,
                    vec![],
                    r#"<input name="password" value="p@ss&w=rd">"#.into(),
                ),
            },
            || {
                let mut books = vec![book(1, "u1", None)];
                let e = format!(
                    "{:#}",
                    block(cw("reject.test").enrich_series(&mut books)).unwrap_err()
                );
                assert!(e.contains("did not start a session"), "{e}");
                assert!(!e.contains("p@ss") && !e.contains("anon-secret"), "{e}");
            },
        );
    }

    #[test]
    fn a_server_that_ignores_offset_stops_at_the_page_cap() {
        let calls = serve(
            |method, path, _, _| match (method, path.split('?').next().unwrap()) {
                ("GET", "/login") => (200, vec![], String::new()),
                ("POST", "/login") => (302, cookie("session=a"), String::new()),
                _ => (200, vec![], rows(0, usize::MAX, 1)),
            },
            || {
                let mut books = vec![book(1, "u1", None)];
                let e = block(cw("endless.test").enrich_series(&mut books)).unwrap_err();
                assert!(e.to_string().contains("pages"), "{e}");
            },
        );
        assert_eq!(calls.len(), 2 + MAX_PAGES);
    }

    #[test]
    fn csrf_tokens_are_read_from_the_login_form() {
        assert_eq!(csrf_token(LOGIN_PAGE).as_deref(), Some("tok-1"));
        assert_eq!(
            csrf_token(r#"<input value="t2" type="hidden" name="csrf_token" />"#).as_deref(),
            Some("t2")
        );
        assert_eq!(csrf_token("<form></form>"), None);
    }

    #[test]
    fn indexes_render_like_the_opds_series_line() {
        let r = |v: serde_json::Value| serde_json::from_value::<Index>(v).unwrap().render();
        assert_eq!(r(json!(3.0)).as_deref(), Some("3"));
        assert_eq!(r(json!(1.5)).as_deref(), Some("1.5"));
        assert_eq!(r(json!("2")).as_deref(), Some("2"));
        assert_eq!(r(json!("")), None);
    }
}
