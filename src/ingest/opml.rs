use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpmlError {
    #[error("invalid OPML")]
    Invalid,
    #[error("too many feeds")]
    TooLarge,
}

pub fn parse_opml(input: &str) -> Result<Vec<(String, String, Option<String>)>, OpmlError> {
    if input.len() > 4 * 1024 * 1024 {
        return Err(OpmlError::TooLarge);
    }
    let mut feeds = Vec::new();
    let mut folder_stack: Vec<String> = Vec::new();
    let mut rest = input;
    while let Some(pos) = rest.find("<outline") {
        rest = &rest[pos..];
        let end = rest.find('>').ok_or(OpmlError::Invalid)?;
        let tag = &rest[..=end];
        let selfclose = tag.ends_with("/>");
        let attrs = Attrs::parse(tag);
        if let Some(url) = attrs.get("xmlUrl").or_else(|| attrs.get("xmlurl")) {
            feeds.push((
                attrs
                    .get("title")
                    .or_else(|| attrs.get("text"))
                    .cloned()
                    .unwrap_or_else(|| url.clone()),
                url.clone(),
                folder_stack.last().cloned(),
            ));
        } else if let Some(title) = attrs.get("title").or_else(|| attrs.get("text")) {
            if !selfclose {
                folder_stack.push(title.clone());
            }
        }
        rest = &rest[end + 1..];
        if !selfclose {
            continue;
        }
        while let Some(close) = rest.find("</outline>") {
            if close == 0 {
                folder_stack.pop();
                rest = &rest[10..];
            } else {
                break;
            }
        }
    }
    if feeds.len() > 10_000 {
        return Err(OpmlError::TooLarge);
    }
    Ok(feeds)
}

pub fn export_opml(feeds: &[(String, String, Option<String>)]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><opml version="2.0"><head><title>myrss subscriptions</title></head><body>"#,
    );
    let mut current: Option<String> = None;
    for (title, url, folder) in feeds {
        if current.as_ref() != folder.as_ref() {
            if current.is_some() {
                out.push_str("</outline>");
            }
            if let Some(f) = folder {
                out.push_str(&format!("<outline text=\"{}\">", esc(f)));
            }
            current = folder.clone();
        }
        out.push_str(&format!(
            "<outline type=\"rss\" text=\"{}\" title=\"{}\" xmlUrl=\"{}\"/>",
            esc(title),
            esc(title),
            esc(url)
        ));
    }
    if current.is_some() {
        out.push_str("</outline>");
    }
    out.push_str("</body></opml>");
    out
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
struct Attrs(std::collections::HashMap<String, String>);
impl Attrs {
    fn parse(tag: &str) -> Self {
        let mut m = std::collections::HashMap::new();
        let mut s = tag;
        while let Some(eq) = s.find('=') {
            let name_start = s[..eq]
                .rfind(|c: char| c.is_whitespace() || c == '<')
                .map(|p| p + 1)
                .unwrap_or(0);
            let name = s[name_start..eq].to_ascii_lowercase();
            s = &s[eq + 1..];
            let quote = s.chars().next();
            if quote == Some('"') {
                s = &s[1..];
                let end = s.find('"').unwrap_or(s.len());
                m.insert(name, s[..end].to_string());
                s = &s[end..];
            } else {
                s = &s[1..];
            }
        }
        Self(m)
    }
    fn get(&self, k: &str) -> Option<&String> {
        self.0.get(&k.to_ascii_lowercase())
    }
}
