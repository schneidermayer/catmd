use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

const DEFAULT_THEME: &str = "base16-ocean.dark";

static SYNTAXES: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEMES: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

#[derive(Debug, Default, Clone, Copy)]
struct InlineStyle {
    strong: usize,
    emphasis: usize,
    strikethrough: usize,
    heading_level: Option<HeadingLevel>,
}

impl InlineStyle {
    fn ansi_prefix(self) -> Option<String> {
        let mut codes = Vec::new();

        if let Some(level) = self.heading_level {
            codes.extend(heading_codes(level));
        }

        if self.strong > 0 {
            codes.push("1");
        }

        if self.emphasis > 0 {
            codes.push("3");
        }

        if self.strikethrough > 0 {
            codes.push("9");
        }

        if codes.is_empty() {
            None
        } else {
            Some(format!("\x1b[{}m", codes.join(";")))
        }
    }
}

fn heading_codes(level: HeadingLevel) -> &'static [&'static str] {
    match level {
        HeadingLevel::H1 => &["1", "4", "38;5;45"],
        HeadingLevel::H2 => &["1", "38;5;39"],
        HeadingLevel::H3 => &["1", "38;5;44"],
        HeadingLevel::H4 => &["4", "38;5;110"],
        HeadingLevel::H5 => &["38;5;109"],
        HeadingLevel::H6 => &["2", "38;5;103"],
    }
}

#[derive(Debug)]
struct ListState {
    next: u64,
    ordered: bool,
}

impl ListState {
    fn new(start: Option<u64>) -> Self {
        Self {
            next: start.unwrap_or(1),
            ordered: start.is_some(),
        }
    }

    fn marker(&mut self) -> String {
        if self.ordered {
            let marker = format!("{}. ", self.next);
            self.next += 1;
            marker
        } else {
            "- ".to_owned()
        }
    }
}

#[derive(Debug)]
struct CodeBlockBuffer {
    language: Option<String>,
    content: String,
}

pub fn render_markdown(input: &str, theme_name: &str) -> String {
    let parser = Parser::new_ext(input, markdown_options());
    let mut out = String::new();

    let mut inline = InlineStyle::default();
    let mut list_stack: Vec<ListState> = Vec::new();
    let mut link_targets: Vec<String> = Vec::new();
    let mut code_block: Option<CodeBlockBuffer> = None;

    for event in parser {
        if code_block.is_some() {
            let mut finished_code_block = false;

            {
                let buffer = code_block.as_mut().expect("code block is checked above");

                match event {
                    Event::End(Tag::CodeBlock(_)) => {
                        out.push_str(&render_code_block(
                            &buffer.content,
                            buffer.language.as_deref(),
                            theme_name,
                        ));

                        if !out.ends_with('\n') {
                            out.push('\n');
                        }

                        out.push('\n');
                        finished_code_block = true;
                    }
                    Event::Text(text) | Event::Code(text) | Event::Html(text) => {
                        buffer.content.push_str(&text)
                    }
                    Event::SoftBreak | Event::HardBreak => buffer.content.push('\n'),
                    _ => {}
                }
            }

            if finished_code_block {
                code_block = None;
            }

            continue;
        }

        match event {
            Event::Start(tag) => match tag {
                Tag::Heading(level, ..) => {
                    ensure_blank_line(&mut out);
                    inline.heading_level = Some(level);
                }
                Tag::Strong => inline.strong += 1,
                Tag::Emphasis => inline.emphasis += 1,
                Tag::Strikethrough => inline.strikethrough += 1,
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(lang) if !lang.trim().is_empty() => {
                            Some(lang.to_string())
                        }
                        _ => None,
                    };

                    code_block = Some(CodeBlockBuffer {
                        language,
                        content: String::new(),
                    });
                }
                Tag::List(start) => list_stack.push(ListState::new(start)),
                Tag::Item => {
                    let depth = list_stack.len().saturating_sub(1);
                    out.push_str(&"  ".repeat(depth));

                    if let Some(list) = list_stack.last_mut() {
                        out.push_str(&list.marker());
                    } else {
                        out.push_str("- ");
                    }
                }
                Tag::Link(_, destination, _) => {
                    link_targets.push(destination.to_string());
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                Tag::Heading(..) => {
                    inline.heading_level = None;
                    ensure_blank_line(&mut out);
                }
                Tag::Strong => inline.strong = inline.strong.saturating_sub(1),
                Tag::Emphasis => inline.emphasis = inline.emphasis.saturating_sub(1),
                Tag::Strikethrough => inline.strikethrough = inline.strikethrough.saturating_sub(1),
                Tag::Paragraph => out.push('\n'),
                Tag::Item => out.push('\n'),
                Tag::List(_) => {
                    list_stack.pop();

                    if list_stack.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                Tag::Link(..) => {
                    if let Some(target) = link_targets.pop() {
                        out.push_str("\x1b[2m (");
                        out.push_str(&target);
                        out.push_str(")\x1b[0m");
                    }
                }
                _ => {}
            },
            Event::Text(text) => push_styled_text(&mut out, &text, inline),
            Event::Code(text) => {
                out.push_str("\x1b[48;5;236m\x1b[38;5;223m ");
                out.push_str(&text);
                out.push_str(" \x1b[0m");
            }
            Event::Rule => {
                out.push_str("\x1b[38;5;244m----------------------------------------\x1b[0m\n")
            }
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::TaskListMarker(checked) => {
                if checked {
                    out.push_str("[x] ");
                } else {
                    out.push_str("[ ] ");
                }
            }
            Event::Html(text) => push_styled_text(&mut out, &text, inline),
            Event::FootnoteReference(name) => {
                out.push('[');
                out.push('^');
                out.push_str(&name);
                out.push(']');
            }
        }
    }

    out
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options
}

fn push_styled_text(out: &mut String, text: &str, style: InlineStyle) {
    if let Some(prefix) = style.ansi_prefix() {
        out.push_str(&prefix);
        out.push_str(text);
        out.push_str("\x1b[0m");
    } else {
        out.push_str(text);
    }
}

fn ensure_blank_line(out: &mut String) {
    if out.is_empty() {
        return;
    }

    let trailing_newlines = out
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\n')
        .count();

    match trailing_newlines {
        0 => out.push_str("\n\n"),
        1 => out.push('\n'),
        _ => {}
    }
}

fn render_code_block(code: &str, language: Option<&str>, theme_name: &str) -> String {
    let syntax = language
        .and_then(|lang| SYNTAXES.find_syntax_by_token(lang))
        .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text());

    let theme = THEMES
        .themes
        .get(theme_name)
        .or_else(|| THEMES.themes.get(DEFAULT_THEME))
        .or_else(|| THEMES.themes.values().next());

    let Some(theme) = theme else {
        return code.to_owned();
    };

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut out = String::new();

    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, &SYNTAXES) {
            Ok(ranges) => out.push_str(&as_24_bit_terminal_escaped(&ranges, false)),
            Err(_) => out.push_str(line),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading_with_ansi() {
        let rendered = render_markdown("# Hello", DEFAULT_THEME);
        assert!(rendered.contains("\x1b[1;4;38;5;45mHello\x1b[0m"));
    }

    #[test]
    fn renders_inline_code() {
        let rendered = render_markdown("Use `catmd`", DEFAULT_THEME);
        assert!(rendered.contains("catmd"));
        assert!(rendered.contains("\x1b[48;5;236m"));
    }

    #[test]
    fn headings_are_surrounded_by_blank_lines() {
        let rendered = render_markdown("before\n\n# Title\n\nafter", DEFAULT_THEME);
        assert!(rendered.contains("before\n\n\x1b[1;4;38;5;45mTitle\x1b[0m\n\nafter"));
    }

    #[test]
    fn heading_levels_have_distinct_styles() {
        let rendered = render_markdown("# One\n## Two\n### Three", DEFAULT_THEME);

        assert!(rendered.contains("\x1b[1;4;38;5;45mOne\x1b[0m"));
        assert!(rendered.contains("\x1b[1;38;5;39mTwo\x1b[0m"));
        assert!(rendered.contains("\x1b[1;38;5;44mThree\x1b[0m"));
    }

    #[test]
    fn list_state_resets_between_separate_lists() {
        let rendered = render_markdown(
            "1. first\n2. second\n\nbreak\n\n1. third\n2. fourth\n",
            DEFAULT_THEME,
        );

        assert!(rendered.contains("break\n1. third\n2. fourth\n"));
        assert!(!rendered.contains("break\n  1. third"));
    }
}
