use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([\w-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s/]+))"#).unwrap());

#[derive(Clone, Debug)]
pub struct Tag {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub start: usize,
    pub end: usize,
    pub closing: bool,
    pub self_closing: bool,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub tag: Tag,
    pub inner: String,
    pub end: usize,
}

pub fn decode(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

pub fn decode_uri_path(text: &str) -> String {
    let mut decoded = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_string())
}

pub fn tags(markup: &str) -> Vec<Tag> {
    let bytes = markup.as_bytes();
    let mut position = 0;
    let mut result = Vec::new();
    while position < bytes.len() {
        if markup[position..].starts_with("```") {
            position += 3;
            position = markup[position..]
                .find("```")
                .map_or(bytes.len(), |end| position + end + 3);
            continue;
        }
        if markup[position..].starts_with("<!--") {
            position = markup[position + 4..]
                .find("-->")
                .map_or(bytes.len(), |end| position + 4 + end + 3);
            continue;
        }
        if bytes[position] == b'`' {
            let delimiter = if markup[position..].starts_with("``") {
                "``"
            } else {
                "`"
            };
            position += delimiter.len();
            position = markup[position..]
                .find(delimiter)
                .map_or(bytes.len(), |end| position + end + delimiter.len());
            continue;
        }
        if bytes[position] != b'<' {
            position += markup[position..].chars().next().unwrap().len_utf8();
            continue;
        }
        let start = position;
        position += 1;
        let mut quote = None;
        while position < bytes.len() {
            let byte = bytes[position];
            if let Some(delimiter) = quote {
                if byte == delimiter {
                    quote = None;
                }
            } else if matches!(byte, b'\'' | b'"') {
                quote = Some(byte);
            } else if byte == b'>' {
                break;
            }
            position += 1;
        }
        if position == bytes.len() {
            break;
        }
        let raw = &markup[start + 1..position];
        position += 1;
        let closing = raw.starts_with('/');
        let raw = raw.trim_start_matches('/').trim_start();
        let name_end = raw
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(raw.len());
        if name_end == 0 {
            continue;
        }
        let name = raw[..name_end].to_ascii_lowercase();
        let attrs = ATTRIBUTE
            .captures_iter(&raw[name_end..])
            .filter_map(|cap| {
                let value = cap.get(2).or(cap.get(3)).or(cap.get(4))?;
                Some((cap[1].to_ascii_lowercase(), decode(value.as_str())))
            })
            .collect();
        let tag = Tag {
            name: name.clone(),
            attrs,
            start,
            end: position,
            closing,
            self_closing: raw.trim_end().ends_with('/'),
        };
        if !closing && matches!(name.as_str(), "code" | "output" | "pre") {
            let close = format!("</{}>", name);
            position = markup[position..]
                .to_ascii_lowercase()
                .find(&close)
                .map_or(bytes.len(), |end| position + end + close.len());
            continue;
        }
        result.push(tag);
    }
    result
}

pub fn blocks(markup: &str, name: &str) -> Vec<Block> {
    let tags = tags(markup);
    let mut stack = Vec::new();
    let mut blocks = Vec::new();
    for tag in tags.into_iter().filter(|tag| tag.name == name) {
        if tag.closing {
            if let Some(open) = stack.pop() {
                let open: Tag = open;
                blocks.push(Block {
                    inner: markup[open.end..tag.start].to_string(),
                    tag: open,
                    end: tag.end,
                });
            }
        } else if tag.self_closing {
            blocks.push(Block {
                end: tag.end,
                inner: String::new(),
                tag,
            });
        } else {
            stack.push(tag);
        }
    }
    blocks.sort_by_key(|block| block.tag.start);
    blocks
}

pub fn line_at(markup: &str, offset: usize) -> usize {
    markup[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

/// Select outer declarations, excluding nested callback argument contracts.
pub fn outer_blocks(markup: &str, name: &str) -> Vec<Block> {
    let mut outer = Vec::new();
    let mut end = 0;
    for block in blocks(markup, name) {
        if block.tag.start >= end {
            end = block.end;
            outer.push(block);
        }
    }
    outer
}

pub fn line_range(value: &str) -> Option<(usize, usize)> {
    let mut parts = value.trim().split('-');
    let first = parts
        .next()?
        .trim_start_matches('L')
        .parse::<usize>()
        .ok()?;
    let last = parts
        .next()
        .map(|part| part.trim_start_matches('L').parse::<usize>().ok())
        .unwrap_or(Some(first))?;
    (parts.next().is_none() && first > 0 && last >= first).then_some((first, last))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_attributes_and_verbatim_examples_do_not_corrupt_structure() {
        let text = "<function parent='util' name='Test'><args><arg type=\"table<string>\" name='data'>x</arg></args><file other='x' line='2-L5'>lua/test.lua</file><code><page>Missing</page><file line='0'>bad</file></code></function>\n```\n<file>bad</file>\n```";
        let function = blocks(text, "function").remove(0);
        assert_eq!(function.tag.attrs["parent"], "util");
        assert_eq!(
            blocks(&function.inner, "arg")[0].tag.attrs["type"],
            "table<string>"
        );
        assert_eq!(blocks(text, "file").len(), 1);
        assert!(blocks(text, "page").is_empty());
    }
    #[test]
    fn ranges_reject_zero_inverted_and_non_numeric_values() {
        assert_eq!(line_range("12-L20"), Some((12, 20)));
        assert_eq!(line_range("L5"), Some((5, 5)));
        for bad in ["0", "20-L12", "-2", "1-L2-L3", "banana"] {
            assert!(line_range(bad).is_none());
        }
    }
}
