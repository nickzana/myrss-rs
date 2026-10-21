use chrono::{DateTime, Utc};
use feed_rs::parser;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid feed")]
    Invalid,
}

pub struct ParsedEntry {
    pub guid: String,
    pub url: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
}

pub fn parse_feed(
    bytes: &[u8],
    _base: &str,
    content_type: Option<&str>,
) -> Result<(String, Option<String>, Vec<ParsedEntry>), ParseError> {
    let feed = parser::parse(bytes).map_err(|_| ParseError::Invalid)?;
    let title = feed
        .title
        .map(|t| t.content)
        .unwrap_or_else(|| "Untitled feed".into());
    let site_url = feed
        .links
        .first()
        .map(|l| l.href.clone())
        .filter(|v| Url::parse(v).is_ok());
    let items = feed
        .entries
        .into_iter()
        .map(|e| {
            let url = e
                .links
                .iter()
                .find_map(|l| Url::parse(&l.href).ok().map(|u| u.to_string()));
            let content = e.content.and_then(|c| c.body.or(c.src.map(|s| s.href)));
            let summary = e.summary.map(|s| s.content);
            let guid = e.id.trim().to_string();
            let guid = if guid.is_empty() {
                url.clone().unwrap_or_else(|| {
                    format!(
                        "{:x}",
                        Sha256::digest(format!(
                            "{}{:?}",
                            e.title.as_ref().map(|t| t.content.as_str()).unwrap_or(""),
                            e.updated
                        ))
                    )
                })
            } else {
                guid
            };
            ParsedEntry {
                guid,
                url,
                title: e
                    .title
                    .map(|t| t.content)
                    .unwrap_or_else(|| "Untitled".into()),
                author: e.authors.first().map(|a| a.name.clone()),
                summary,
                content,
                published_at: e.published.or(e.updated),
            }
        })
        .collect();
    let _ = content_type;
    Ok((title, site_url, items))
}

pub fn discover_feed_url(html: &str, base: &Url) -> Option<Url> {
    let lower = html.to_ascii_lowercase();
    let mut start = 0;
    while let Some(pos) = lower[start..].find("<link") {
        let pos = start + pos;
        let end = lower[pos..].find('>').map(|e| pos + e)?;
        let tag = &lower[pos..=end];
        let ty = tag
            .split("type=\"")
            .nth(1)
            .and_then(|v| v.split('\"').next())
            .unwrap_or("");
        if ty.contains("rss") || ty.contains("atom") || ty.contains("json") {
            if let Some(href) = tag
                .split("href=\"")
                .nth(1)
                .and_then(|v| v.split('\"').next())
            {
                return base.join(href).ok();
            }
        }
        start = end + 1;
    }
    None
}
