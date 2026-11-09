//! A small XML tokenizer for the parts of Office files: tags with the byte
//! ranges of their attribute values, and text, so that a value can be
//! replaced in place and everything else stays byte for byte. OOXML parts are
//! well-formed XML without a DTD; what does not tokenize is an error.

use std::ops::Range;

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// `<name ...>`, `</name>` or `<name .../>`.
    Tag {
        name: String,
        kind: TagKind,
        /// The whole tag.
        range: Range<usize>,
        /// Each attribute: its name and the range of its value (without
        /// quotes, still escaped).
        attrs: Vec<(String, Range<usize>)>,
    },
    /// Character data between tags, still escaped.
    Text(Range<usize>),
    /// The content of `<![CDATA[...]]>`.
    CData(Range<usize>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    Start,
    End,
    Empty,
}

fn bad(at: usize, what: &str) -> Error {
    Error::Format(format!("XML at byte {at}: {what}"))
}

/// The tokens of `s`; comments, processing instructions and declarations
/// are skipped.
pub fn tokenize(s: &str) -> Result<Vec<Token>, Error> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let end = b[i..]
                .iter()
                .position(|c| *c == b'<')
                .map_or(b.len(), |p| i + p);
            out.push(Token::Text(i..end));
            i = end;
            continue;
        }
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").ok_or_else(|| bad(i, "an open comment"))? + 3;
        } else if rest.starts_with("<![CDATA[") {
            let end = rest
                .find("]]>")
                .ok_or_else(|| bad(i, "an open CDATA section"))?;
            out.push(Token::CData(i + 9..i + end));
            i += end + 3;
        } else if rest.starts_with("<?") {
            i += rest
                .find("?>")
                .ok_or_else(|| bad(i, "an open processing instruction"))?
                + 2;
        } else if rest.starts_with("<!") {
            i += rest
                .find('>')
                .ok_or_else(|| bad(i, "an open declaration"))?
                + 1;
        } else {
            let start = i;
            let end_tag = b.get(i + 1) == Some(&b'/');
            let mut j = i + if end_tag { 2 } else { 1 };
            let name_start = j;
            while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
                j += 1;
            }
            let name = s[name_start..j].to_owned();
            if name.is_empty() {
                return Err(bad(i, "a tag without a name"));
            }
            let mut attrs = Vec::new();
            loop {
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                match b.get(j) {
                    None => return Err(bad(start, "an open tag")),
                    Some(b'>') => {
                        out.push(Token::Tag {
                            name,
                            kind: if end_tag {
                                TagKind::End
                            } else {
                                TagKind::Start
                            },
                            range: start..j + 1,
                            attrs,
                        });
                        i = j + 1;
                        break;
                    }
                    Some(b'/') if b.get(j + 1) == Some(&b'>') => {
                        out.push(Token::Tag {
                            name,
                            kind: TagKind::Empty,
                            range: start..j + 2,
                            attrs,
                        });
                        i = j + 2;
                        break;
                    }
                    Some(_) => {
                        let a = j;
                        while j < b.len()
                            && b[j] != b'='
                            && !b[j].is_ascii_whitespace()
                            && b[j] != b'>'
                        {
                            j += 1;
                        }
                        let attr = s[a..j].to_owned();
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if b.get(j) != Some(&b'=') {
                            return Err(bad(j, "an attribute without a value"));
                        }
                        j += 1;
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        let q = *b.get(j).ok_or_else(|| bad(j, "an open tag"))?;
                        if q != b'"' && q != b'\'' {
                            return Err(bad(j, "an unquoted attribute"));
                        }
                        let v = j + 1;
                        let close = b[v..]
                            .iter()
                            .position(|c| *c == q)
                            .ok_or_else(|| bad(j, "an open attribute"))?
                            + v;
                        attrs.push((attr, v..close));
                        j = close + 1;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// `s` with the five XML entities and character references decoded, and
/// where each escape ended: (decoded offset, raw offset) pairs, as the
/// engine's segments keep them.
pub fn unescape(s: &str, base: usize) -> (String, Vec<(usize, usize)>) {
    let mut out = String::with_capacity(s.len());
    let mut marks = vec![(0, base)];
    let mut i = 0;
    while let Some(p) = s[i..].find('&') {
        out.push_str(&s[i..i + p]);
        let at = i + p;
        let Some(semi) = s[at..].find(';').filter(|n| *n <= 10) else {
            out.push('&');
            i = at + 1;
            continue;
        };
        let entity = &s[at + 1..at + semi];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            // HTML's, for mail bodies (ADR-0020).
            "nbsp" => Some(' '),
            "auml" => Some('ä'),
            "ouml" => Some('ö'),
            "uuml" => Some('ü'),
            "Auml" => Some('Ä'),
            "Ouml" => Some('Ö'),
            "Uuml" => Some('Ü'),
            "szlig" => Some('ß'),
            "eacute" => Some('é'),
            "euro" => Some('€'),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                i = at + semi + 1;
                marks.push((out.len(), base + i));
            }
            None => {
                out.push('&');
                i = at + 1;
            }
        }
    }
    out.push_str(&s[i..]);
    (out, marks)
}

/// `s` escaped for text (`attr`: for an attribute value in double quotes).
pub fn escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The local name of a tag or attribute (`w:t` -> `t`).
pub fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_ranges() {
        let s = r#"<?xml version="1.0"?><w:p a="1" b='x &amp; y'><w:t xml:space="preserve">Hi &lt;3</w:t><br/><!-- c --></w:p>"#;
        let t = tokenize(s).unwrap();
        let Token::Tag { name, attrs, .. } = &t[0] else {
            panic!()
        };
        assert_eq!(name, "w:p");
        assert_eq!(&s[attrs[1].1.clone()], "x &amp; y");
        let Token::Text(r) = &t[2] else {
            panic!("{t:?}")
        };
        assert_eq!(&s[r.clone()], "Hi &lt;3");
        assert!(matches!(
            &t[4],
            Token::Tag {
                kind: TagKind::Empty,
                ..
            }
        ));
        assert!(tokenize("<a b=c>").is_err());
        assert!(tokenize("<a").is_err());
    }

    #[test]
    fn escapes_round_trip() {
        let (u, marks) = unescape("a &amp; b &#x41;&#66;", 10);
        assert_eq!(u, "a & b AB");
        assert_eq!(marks[1], (3, 10 + 7));
        assert_eq!(escape("<a & \"b\">", true), "&lt;a &amp; &quot;b&quot;&gt;");
        assert_eq!(local("w:t"), "t");
    }
}
