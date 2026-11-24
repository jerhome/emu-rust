//! Tiny Markdown subset used by the built-in manual. Pure and total: malformed input never panics,
//! unknown syntax degrades to plain text.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Span {
    Text(String),
    Bold(String),
    Code(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Paragraph(Vec<Span>),
    List {
        ordered: bool,
        items: Vec<Vec<Span>>,
    },
    Code(String),
    Table {
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
}

/// Plain text of a span run (used for anchors and titles).
pub(crate) fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Bold(t) | Span::Code(t) => t.as_str(),
        })
        .collect()
}

/// Inline parsing: `**bold**` and `` `code` ``; an unclosed marker stays literal.
pub(crate) fn parse_inline(s: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = s;
    let flush = |text: &mut String, out: &mut Vec<Span>| {
        if !text.is_empty() {
            out.push(Span::Text(std::mem::take(text)));
        }
    };
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**")
            && let Some(end) = after.find("**")
            && end > 0
        {
            flush(&mut text, &mut out);
            out.push(Span::Bold(after[..end].to_string()));
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
            && end > 0
        {
            flush(&mut text, &mut out);
            out.push(Span::Code(after[..end].to_string()));
            rest = &after[end + 1..];
        } else {
            let mut chars = rest.chars();
            if let Some(c) = chars.next() {
                text.push(c);
            }
            rest = chars.as_str();
        }
    }
    flush(&mut text, &mut out);
    out
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    let rest = line[hashes..].strip_prefix(' ')?;
    (1..=6)
        .contains(&hashes)
        .then(|| (hashes as u8, rest.trim()))
}

fn bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
}

fn numbered(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    t[digits..].strip_prefix(". ")
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.len() > 1 && t.starts_with('|')
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    is_table_row(t) && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) && t.contains('-')
}

fn cells(line: &str) -> Vec<Vec<Span>> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    // A `|` inside a code span is not supported by the subset; the manual does not use it.
    t.split('|').map(|c| parse_inline(c.trim())).collect()
}

pub(crate) fn parse(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
        } else if line.trim_start().starts_with("```") {
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push(lines[i]);
                i += 1;
            }
            i += 1; // closing fence (or end of input for an unclosed block)
            blocks.push(Block::Code(code.join("\n")));
        } else if let Some((level, title)) = heading(line) {
            blocks.push(Block::Heading {
                level,
                spans: parse_inline(title),
            });
            i += 1;
        } else if is_table_row(line) && lines.get(i + 1).is_some_and(|l| is_separator(l)) {
            let header = cells(line);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && is_table_row(lines[i]) {
                rows.push(cells(lines[i]));
                i += 1;
            }
            blocks.push(Block::Table { header, rows });
        } else if bullet(line).is_some() || numbered(line).is_some() {
            let ordered = numbered(line).is_some();
            let mut items: Vec<Vec<Span>> = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                let item = if ordered { numbered(l) } else { bullet(l) };
                if let Some(item) = item {
                    items.push(parse_inline(item.trim()));
                } else if !l.trim().is_empty()
                    && l.starts_with(' ')
                    && heading(l).is_none()
                    && let Some(last) = items.last_mut()
                {
                    // Continuation line of the previous item.
                    last.push(Span::Text(" ".into()));
                    last.extend(parse_inline(l.trim()));
                } else {
                    break;
                }
                i += 1;
            }
            blocks.push(Block::List { ordered, items });
        } else {
            let mut para = String::new();
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty()
                    || heading(l).is_some()
                    || l.trim_start().starts_with("```")
                    || bullet(l).is_some()
                    || numbered(l).is_some()
                    || is_table_row(l)
                {
                    break;
                }
                if !para.is_empty() {
                    para.push(' ');
                }
                para.push_str(l.trim());
                i += 1;
            }
            if para.is_empty() {
                // Defensive: a line no rule accepts is kept as text, and the cursor always advances.
                para = line.trim().to_string();
                i += 1;
            }
            blocks.push(Block::Paragraph(parse_inline(&para)));
        }
    }
    blocks
}

/// Anchor of a title: lowercase ASCII, accents folded, words joined by `-`, leading "N." number dropped.
pub(crate) fn anchor(title: &str) -> String {
    let t = title.trim();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    let t = t[digits..].strip_prefix('.').map_or(t, str::trim_start);
    let mut out = String::new();
    for c in t.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Span {
        Span::Text(s.into())
    }

    #[test]
    fn empty_input_gives_no_blocks() {
        assert!(parse("").is_empty());
        assert!(parse("\n  \n").is_empty());
    }

    #[test]
    fn headings_and_paragraphs() {
        let b = parse("# Titre\n\nUne ligne\nsuite.\n\n### Sous");
        assert_eq!(
            b,
            vec![
                Block::Heading {
                    level: 1,
                    spans: vec![t("Titre")]
                },
                Block::Paragraph(vec![t("Une ligne suite.")]),
                Block::Heading {
                    level: 3,
                    spans: vec![t("Sous")]
                },
            ]
        );
    }

    #[test]
    fn inline_bold_code_and_unclosed_markers() {
        assert_eq!(
            parse_inline("a **b** `c` d"),
            vec![
                t("a "),
                Span::Bold("b".into()),
                t(" "),
                Span::Code("c".into()),
                t(" d")
            ]
        );
        assert_eq!(parse_inline("**ouvert `x"), vec![t("**ouvert `x")]);
        assert_eq!(parse_inline("é**"), vec![t("é**")]);
    }

    #[test]
    fn lists_bulleted_and_numbered() {
        let b = parse("- un\n- deux\n  suite\n\n1. a\n2. b");
        assert_eq!(
            b,
            vec![
                Block::List {
                    ordered: false,
                    items: vec![vec![t("un")], vec![t("deux"), t(" "), t("suite")]]
                },
                Block::List {
                    ordered: true,
                    items: vec![vec![t("a")], vec![t("b")]]
                },
            ]
        );
    }

    #[test]
    fn code_block_even_unclosed() {
        assert_eq!(
            parse("```\na\n  b\n```"),
            vec![Block::Code("a\n  b".into())]
        );
        assert_eq!(parse("```\nx"), vec![Block::Code("x".into())]);
    }

    #[test]
    fn table_with_header() {
        let b = parse("| A | B |\n|---|---|\n| `x` | y |\n| 1 | 2 |");
        let Block::Table { header, rows } = &b[0] else {
            panic!("table attendu")
        };
        assert_eq!(header.len(), 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], vec![Span::Code("x".into())]);
    }

    #[test]
    fn table_without_separator_is_text() {
        let b = parse("| a | b |");
        assert!(matches!(b[0], Block::Paragraph(_)));
    }

    #[test]
    fn malformed_input_never_panics() {
        for s in [
            "#",
            "# ",
            "####### x",
            "|",
            "||",
            "|-|",
            "**",
            "`",
            "-",
            "- ",
            "1.",
            "1. ",
            "```",
            "é\u{0}ü|",
            "|a|\n|-|",
        ] {
            let _ = parse(s);
        }
    }

    #[test]
    fn anchors() {
        assert_eq!(anchor("4. Clavier"), "clavier");
        assert_eq!(anchor("1. Premiers pas"), "premiers-pas");
        assert_eq!(anchor("7. Problèmes fréquents"), "problemes-frequents");
        assert_eq!(
            anchor("Ce que l'application télécharge"),
            "ce-que-l-application-telecharge"
        );
        assert_eq!(anchor("???"), "");
    }
}
