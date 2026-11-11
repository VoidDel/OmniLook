//! Markdown (`docs/development-plan.md` §5.1, §8): rendered by `pulldown-cmark` (GitHub's
//! extensions: tables, task lists, strikethrough, footnotes), code blocks coloured like the text
//! preview, then cleaned by `ammonia`: no script, style, frames, forms or event handlers; links
//! only to web and mail addresses. Images on this computer go through the window's own protocol
//! (the caller says how); images from the internet are never loaded and become links instead.

use std::collections::{HashMap, HashSet};

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

use crate::text;

/// Prefix of every `id` in the rendered page, so the file's ids cannot clash with the window's
/// own. In-page links (`#section`) need it added (the page does that).
pub const ID_PREFIX: &str = "md-";

fn remote(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("//")
}

/// GitHub's heading anchors: lower case, spaces to `-`, punctuation dropped (letters of any script
/// kept); repeats get `-1`, `-2`.
fn slug(text: &str, seen: &mut HashMap<String, usize>) -> String {
    let base: String = text
        .trim()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    let n = seen.entry(base.clone()).or_insert(0);
    let s = if *n == 0 { base } else { format!("{base}-{n}") };
    *n += 1;
    s
}

/// The page for `markdown`. `local_image` turns an image address that is not on the internet
/// (`img/a.png`, `../shot.jpg`, `C:\x.png`) into one the window can load, or `None` to drop it.
pub fn render(markdown: &str, local_image: impl Fn(&str) -> Option<String>) -> String {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let events: Vec<Event> = Parser::new_ext(markdown, opts).collect();
    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    let mut seen = HashMap::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            // Heading anchors, from the heading's text.
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => {
                let mut label = String::new();
                for e in &events[i + 1..] {
                    match e {
                        Event::End(TagEnd::Heading(_)) => break,
                        Event::Text(t) | Event::Code(t) => label.push_str(t),
                        _ => {}
                    }
                }
                let id = id
                    .clone()
                    .unwrap_or_else(|| CowStr::from(slug(&label, &mut seen)));
                out.push(Event::Start(Tag::Heading {
                    level: *level,
                    id: Some(id),
                    classes: classes.clone(),
                    attrs: attrs.clone(),
                }));
            }
            // Code blocks coloured here; the fence's first word names the language.
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or(""),
                    CodeBlockKind::Indented => "",
                };
                let mut code = String::new();
                let mut j = i + 1;
                while j < events.len() {
                    match &events[j] {
                        Event::End(TagEnd::CodeBlock) => break,
                        Event::Text(t) => code.push_str(t),
                        _ => {}
                    }
                    j += 1;
                }
                out.push(Event::Html(CowStr::from(format!(
                    "<pre><code>{}</code></pre>\n",
                    text::highlight_html(lang, &code)
                ))));
                i = j + 1;
                continue;
            }
            // Images: internet ones become links to them (nothing is fetched), local ones go
            // through the window.
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                if remote(dest_url) {
                    out.push(Event::Start(Tag::Link {
                        link_type: *link_type,
                        dest_url: dest_url.clone(),
                        title: title.clone(),
                        id: id.clone(),
                    }));
                    out.push(Event::Text(CowStr::from("🖼 ")));
                    let mut j = i + 1;
                    while j < events.len() && !matches!(events[j], Event::End(TagEnd::Image)) {
                        out.push(events[j].clone());
                        j += 1;
                    }
                    out.push(Event::End(TagEnd::Link));
                    i = j + 1;
                    continue;
                }
                let src = local_image(dest_url).unwrap_or_default();
                out.push(Event::Start(Tag::Image {
                    link_type: *link_type,
                    dest_url: CowStr::from(src),
                    title: title.clone(),
                    id: id.clone(),
                }));
            }
            e => out.push(e.clone()),
        }
        i += 1;
    }
    let mut html = String::with_capacity(markdown.len() * 3 / 2);
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    clean(&html)
}

/// What survives of the HTML: ammonia's safe set, plus the token classes, anchors, task-list
/// boxes and table alignment.
fn clean(html: &str) -> String {
    let tk: Vec<String> = (1..=10).map(|n| format!("tk{n}")).collect();
    let tk: Vec<&str> = tk.iter().map(String::as_str).collect();
    let mut b = ammonia::Builder::default();
    b.add_allowed_classes("span", &tk)
        // Footnotes, as pulldown-cmark marks them (the page styles them).
        .add_allowed_classes("div", &["footnote-definition"])
        .add_allowed_classes("sup", &["footnote-reference", "footnote-definition-label"])
        .add_tags(["input"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_tag_attributes("th", ["style"])
        .add_tag_attributes("td", ["style"])
        .id_prefix(Some(ID_PREFIX))
        // `look:` is the window's own protocol where it keeps its scheme (Linux): the local
        // images' addresses, made by `render`'s caller.
        .url_schemes(HashSet::from(["http", "https", "mailto", "look"]))
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            // Only task-list check boxes, and only alignment as a style.
            ("input", "type") => (value == "checkbox").then(|| value.into()),
            (_, "style") => matches!(
                value.trim().trim_end_matches(';'),
                "text-align: left" | "text-align: center" | "text-align: right"
            )
            .then(|| value.into()),
            _ => Some(value.into()),
        });
    for h in ["h1", "h2", "h3", "h4", "h5", "h6", "div", "li", "sup"] {
        b.add_tag_attributes(h, ["id"]);
    }
    b.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(md: &str) -> String {
        render(md, |p| Some(format!("http://look.localhost/r/t/{p}")))
    }

    #[test]
    fn renders_and_cleans() {
        let h = r(
            "# 标题 One\n\nText **bold** and <script>alert(1)</script> <b onclick=\"x()\">b</b>\n",
        );
        assert!(h.contains("<h1 id=\"md-标题-one\">"), "{h}");
        assert!(h.contains("<strong>bold</strong>"), "{h}");
        assert!(!h.contains("script") && !h.contains("onclick"), "{h}");
        // Raw frames, styles and javascript: links are gone.
        let h = r("<iframe src=\"x\"></iframe><p style=\"color:red\">p</p>

[x](javascript:alert(1))");
        assert!(
            !h.contains("iframe") && !h.contains("color") && !h.contains("javascript"),
            "{h}"
        );
    }

    #[test]
    fn code_tables_tasks_and_anchors() {
        let h = r(
            "```rust\nfn main() {}\n```\n\n| a | b |\n|:-:|--:|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n## Same\n## Same\n",
        );
        assert!(h.contains("<span class=\"tk4\">fn</span>"), "{h}");
        assert!(h.contains("style=\"text-align: center\""), "{h}");
        assert!(
            h.contains("type=\"checkbox\"") && h.contains("checked"),
            "{h}"
        );
        assert!(
            h.contains("id=\"md-same\"") && h.contains("id=\"md-same-1\""),
            "{h}"
        );
        // Code in an unknown language is escaped, not interpreted.
        let h = r("```nolang\n<b>x</b>\n```\n");
        assert!(h.contains("&lt;b&gt;x&lt;/b&gt;"), "{h}");
    }

    #[test]
    fn images() {
        let h = r("![logo](img/a.png) ![remote](https://example.com/x.png)");
        assert!(
            h.contains("<img src=\"http://look.localhost/r/t/img/a.png\" alt=\"logo\""),
            "{h}"
        );
        assert!(!h.contains("<img src=\"https"), "{h}");
        assert!(h.contains("<a href=\"https://example.com/x.png\""), "{h}");
        assert!(h.contains("remote"), "{h}");
    }
}
