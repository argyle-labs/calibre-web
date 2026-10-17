#![allow(clippy::disallowed_types)]

use base64::Engine;
use plugin_toolkit::api_client::ApiClientBuilder;
use plugin_toolkit::prelude::*;
use plugin_toolkit::progenitor_client::QueryParam;
use plugin_toolkit::reqwest::Client as HttpClient;

pub struct Cw {
    base_url: String,
    http: HttpClient,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub id: Option<u64>,
    pub uuid: String,
    pub title: String,
    pub authors: Vec<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub formats: Vec<String>,
    pub languages: Vec<String>,
    pub published: Option<String>,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Page {
    pub books: Vec<Book>,
    pub entries: Vec<CatalogEntry>,
    pub next_offset: Option<usize>,
}

impl Cw {
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self> {
        if username.trim().is_empty() {
            bail!("calibre-web username must not be empty");
        }
        let basic =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        let http = ApiClientBuilder::new()
            .header("authorization", format!("Basic {basic}"))?
            .build()?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn get(&self, path: &str, offset: usize) -> Result<(u16, String)> {
        let mut req = self.http.get(format!("{}{}", self.base_url, path));
        if offset > 0 {
            req = req.query(&QueryParam::new("offset", &offset.to_string()));
        }
        let resp = req.send().await.map_err(|e| anyhow!("GET {path}: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| anyhow!("GET {path}: {e}"))?;
        Ok((status, body))
    }

    async fn feed(&self, path: &str) -> Result<Page> {
        let mut out = Page::default();
        let mut offset = 0;
        loop {
            let (status, body) = self.get(path, offset).await?;
            if !(200..300).contains(&status) {
                bail!("GET {path}: HTTP {status} {}", error_excerpt(&body));
            }
            let page = parse_feed(&body).map_err(|e| anyhow!("GET {path}: {e}"))?;
            let empty = page.books.is_empty() && page.entries.is_empty();
            out.books.extend(page.books);
            out.entries.extend(page.entries);
            match page.next_offset {
                Some(next) if next > offset && !empty => offset = next,
                _ => return Ok(out),
            }
        }
    }

    pub async fn books(&self) -> Result<Vec<Book>> {
        Ok(self.feed("/opds/books/letter/00").await?.books)
    }

    pub async fn series(&self) -> Result<Vec<CatalogEntry>> {
        Ok(self.feed("/opds/series/letter/00").await?.entries)
    }

    pub async fn authors(&self) -> Result<Vec<CatalogEntry>> {
        Ok(self.feed("/opds/author/letter/00").await?.entries)
    }

    pub async fn reconnect(&self) -> Result<()> {
        match self.get("/reconnect", 0).await? {
            (200..=299, _) => Ok(()),
            (404, _) => bail!(
                "GET /reconnect: HTTP 404; the route is off (start calibre-web with -r or CALIBRE_RECONNECT=1)"
            ),
            (status, body) => bail!("GET /reconnect: HTTP {status} {}", error_excerpt(&body)),
        }
    }
}

const ERROR_EXCERPT: usize = 256;

fn error_excerpt(body: &str) -> String {
    let body = body.trim();
    let cut = body
        .char_indices()
        .nth(ERROR_EXCERPT)
        .map_or(body, |(i, _)| &body[..i]);
    plugin_toolkit::logging::scrub(cut).into_owned()
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}

fn children<'a, 'i: 'a>(
    n: roxmltree::Node<'a, 'i>,
    name: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> + 'a {
    n.children().filter(move |c| c.tag_name().name() == name)
}

fn text_of(n: Option<roxmltree::Node>) -> Option<String> {
    n.and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn segment_after(href: &str, marker: &str) -> Option<u64> {
    let rest = &href[href.find(marker)? + marker.len()..];
    rest.split(['/', '?']).next()?.parse().ok()
}

fn last_id(href: &str) -> Option<u64> {
    href.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

fn offset_of(href: &str) -> Option<usize> {
    href.split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix("offset="))?
        .parse()
        .ok()
}

pub fn parse_series_line(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.trim().strip_prefix("SERIES:")?.trim();
    match rest.strip_suffix(']').and_then(|r| r.rsplit_once(" [")) {
        Some((name, idx)) if !name.trim().is_empty() => Some((
            name.trim().to_string(),
            Some(idx.trim().to_string()).filter(|s| !s.is_empty()),
        )),
        _ if !rest.is_empty() => Some((rest.to_string(), None)),
        _ => None,
    }
}

fn parse_book(e: roxmltree::Node, uuid: &str) -> Book {
    let links: Vec<_> = children(e, "link").collect();
    let id = links.iter().find_map(|l| {
        let href = l.attribute("href")?;
        segment_after(href, "/opds/download/").or_else(|| segment_after(href, "/opds/cover/"))
    });
    let formats = links
        .iter()
        .filter(|l| l.attribute("rel") == Some("http://opds-spec.org/acquisition"))
        .filter_map(|l| l.attribute("title"))
        .map(String::from)
        .collect();
    let series = child(e, "content")
        .into_iter()
        .flat_map(|c| c.descendants())
        .filter(|n| n.is_text())
        .find_map(|n| parse_series_line(n.text().unwrap_or_default()));
    Book {
        id,
        uuid: uuid.to_string(),
        title: text_of(child(e, "title")).unwrap_or_default(),
        authors: children(e, "author")
            .filter_map(|a| text_of(child(a, "name")))
            .collect(),
        series_index: series.as_ref().and_then(|s| s.1.clone()),
        series: series.map(|s| s.0),
        formats,
        languages: children(e, "language")
            .filter_map(|l| text_of(Some(l)))
            .collect(),
        published: text_of(child(e, "published"))
            .map(|p| p.split('T').next().unwrap_or_default().to_string()),
    }
}

pub fn parse_feed(xml: &str) -> Result<Page> {
    let doc = roxmltree::Document::parse(xml).map_err(|_| anyhow!("expected an OPDS feed"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "feed" {
        bail!("expected an OPDS feed");
    }
    let mut page = Page {
        next_offset: children(root, "link")
            .find(|l| l.attribute("rel") == Some("next"))
            .and_then(|l| l.attribute("href"))
            .and_then(offset_of),
        ..Page::default()
    };
    for e in children(root, "entry") {
        let id = text_of(child(e, "id")).unwrap_or_default();
        if let Some(uuid) = id.strip_prefix("urn:uuid:") {
            page.books.push(parse_book(e, uuid));
        } else if let Some(n) = last_id(&id) {
            page.entries.push(CatalogEntry {
                id: n,
                name: text_of(child(e, "title")).unwrap_or_default(),
            });
        }
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dcterms="http://purl.org/dc/terms/">
  <link rel="next" href="/cw/opds/books/letter/00?offset=60" type="application/atom+xml"/>
  <entry>
    <title>Oathbringer</title>
    <id>urn:uuid:0b6e-11</id>
    <author><name>Brandon Sanderson</name></author>
    <published>2017-11-14T00:00:00+00:00</published>
    <dcterms:language>eng</dcterms:language>
    <content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml">
      TAGS: Fantasy<br/>
      SERIES: The Stormlight Archive [3]<br/>
      <p>SERIES: not this</p>
    </div></content>
    <link type="image/jpeg" href="/cw/opds/cover/42" rel="http://opds-spec.org/image"/>
    <link rel="http://opds-spec.org/acquisition" href="/cw/opds/download/42/epub/" title="EPUB" type="application/epub+zip"/>
    <link rel="http://opds-spec.org/acquisition" href="/cw/opds/download/42/azw3/" title="AZW3" type="application/x-mobi8-ebook"/>
  </entry>
  <entry>
    <title>The Stormlight Archive</title>
    <id>/cw/opds/series/7</id>
  </entry>
</feed>"#;

    #[test]
    fn feed_maps_books_entries_and_next_offset() {
        let p = parse_feed(FEED).unwrap();
        assert_eq!(p.next_offset, Some(60));
        assert_eq!(
            p.entries,
            vec![CatalogEntry {
                id: 7,
                name: "The Stormlight Archive".into()
            }]
        );
        let b = &p.books[0];
        assert_eq!(b.id, Some(42));
        assert_eq!(b.uuid, "0b6e-11");
        assert_eq!(b.authors, vec!["Brandon Sanderson"]);
        assert_eq!(b.series.as_deref(), Some("The Stormlight Archive"));
        assert_eq!(b.series_index.as_deref(), Some("3"));
        assert_eq!(b.formats, vec!["EPUB", "AZW3"]);
        assert_eq!(b.languages, vec!["eng"]);
        assert_eq!(b.published.as_deref(), Some("2017-11-14"));
    }

    #[test]
    fn series_line_keeps_brackets_in_names_and_fractional_index() {
        assert_eq!(
            parse_series_line(" SERIES: Dune [Saga] [1.5]"),
            Some(("Dune [Saga]".into(), Some("1.5".into())))
        );
        assert_eq!(parse_series_line("TAGS: x"), None);
    }

    #[test]
    fn non_feed_bodies_are_rejected() {
        assert!(parse_feed("<html><body/></html>").is_err());
        assert!(parse_feed("not xml").is_err());
    }
}
