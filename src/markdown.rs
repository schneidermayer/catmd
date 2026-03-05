use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
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
    heading: bool,
}

impl InlineStyle {
    fn ansi_prefix(self) -> Option<String> {
        let mut codes = Vec::new();

        if self.heading {
            codes.push("1");
            codes.push("38;5;39");
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
                Tag::Heading(..) => {
                    inline.heading = true;
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
                    inline.heading = false;
                    out.push('\n');
                }
                Tag::Strong => inline.strong = inline.strong.saturating_sub(1),
                Tag::Emphasis => inline.emphasis = inline.emphasis.saturating_sub(1),
                Tag::Strikethrough => inline.strikethrough = inline.strikethrough.saturating_sub(1),
                Tag::Paragraph => out.push('\n'),
                Tag::Item => out.push('\n'),
                Tag::List(_) => {
                    if !out.ends_with('\n') {
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
        assert!(rendered.contains("\x1b[1;38;5;39mHello\x1b[0m"));
    }

    #[test]
    fn renders_inline_code() {
        let rendered = render_markdown("Use `catmd`", DEFAULT_THEME);
        assert!(rendered.contains("catmd"));
        assert!(rendered.contains("\x1b[48;5;236m"));
    }
}
