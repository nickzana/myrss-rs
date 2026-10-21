use std::collections::HashSet;

pub fn sanitize_html(input: &str, base_url: Option<&str>, allow_remote_media: bool) -> String {
    let mut builder = ammonia::Builder::default();
    builder.tags(allowed_tags());
    builder.generic_attributes(["title", "lang", "dir"].into_iter().collect());
    builder.tag_attributes(
        [
            ("a", ["href", "title"].into_iter().collect()),
            (
                "img",
                ["src", "alt", "title", "width", "height"]
                    .into_iter()
                    .collect(),
            ),
            ("blockquote", ["cite"].into_iter().collect()),
            ("q", ["cite"].into_iter().collect()),
            ("time", ["datetime"].into_iter().collect()),
        ]
        .into_iter()
        .collect(),
    );
    builder.url_schemes(["http", "https", "mailto"].into_iter().collect());
    if !allow_remote_media {
        builder.rm_tags(["img", "picture", "source", "video", "audio"]);
    }
    builder.link_rel(Some("noopener noreferrer nofollow"));
    let cleaned = builder.clean(input).to_string();
    match base_url {
        Some(base) => absolutize(&cleaned, base),
        None => cleaned,
    }
}

fn allowed_tags() -> HashSet<&'static str> {
    [
        "a",
        "abbr",
        "article",
        "aside",
        "b",
        "blockquote",
        "br",
        "caption",
        "cite",
        "code",
        "dd",
        "del",
        "details",
        "div",
        "dl",
        "dt",
        "em",
        "figcaption",
        "figure",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "i",
        "img",
        "ins",
        "kbd",
        "li",
        "mark",
        "ol",
        "p",
        "picture",
        "pre",
        "q",
        "s",
        "section",
        "small",
        "source",
        "span",
        "strong",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "time",
        "tr",
        "u",
        "ul",
        "var",
    ]
    .into_iter()
    .collect()
}

fn absolutize(html: &str, base: &str) -> String {
    // Keep this deliberately conservative: only common href/src attributes are resolved.
    let mut out = String::with_capacity(html.len() + base.len());
    let mut rest = html;
    while let Some(pos) = rest.find(['h', 's']) {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let marker = if rest.starts_with("href=\"") {
            "href=\""
        } else if rest.starts_with("src=\"") {
            "src=\""
        } else {
            out.push(rest.chars().next().unwrap());
            rest = &rest[1..];
            continue;
        };
        out.push_str(marker);
        rest = &rest[marker.len()..];
        let end = rest.find('\"').unwrap_or(rest.len());
        let (value, tail) = rest.split_at(end);
        if !value.starts_with("#") && !value.starts_with("data:") {
            if let Ok(resolved) = url::Url::parse(base).and_then(|b| b.join(value)) {
                out.push_str(resolved.as_str());
            } else {
                out.push_str(value);
            }
        } else {
            out.push_str(value);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_active_content() {
        let out=sanitize_html("<p onclick=x>Hi<script>alert(1)</script><a href=\"javascript:x\">x</a><img src=\"https://e/x.png\">",None,false);
        assert!(
            !out.contains("script")
                && !out.contains("onclick")
                && !out.contains("javascript")
                && !out.contains("<img")
        );
    }
}
