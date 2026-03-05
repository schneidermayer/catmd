use once_cell::sync::Lazy;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
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

#[derive(Debug)]
struct TableState {
    alignments: Vec<Alignment>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_header: bool,
    link_targets: Vec<String>,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: Vec::new(),
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            in_header: false,
            link_targets: Vec::new(),
        }
    }

    fn finish_cell(&mut self) {
        self.current_row.push(self.current_cell.trim().to_owned());
        self.current_cell.clear();
    }

    fn finish_row(&mut self) {
        if self.in_header {
            self.header = std::mem::take(&mut self.current_row);
        } else {
            self.rows.push(std::mem::take(&mut self.current_row));
        }
    }

    fn render(&self) -> String {
        let columns = self
            .alignments
            .len()
            .max(self.header.len())
            .max(self.rows.iter().map(|row| row.len()).max().unwrap_or(0));

        if columns == 0 {
            return String::new();
        }

        let mut widths = vec![3; columns];

        for (index, cell) in self.header.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }

        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }

        let mut out = String::new();

        if !self.header.is_empty() {
            out.push_str(&render_table_row(
                &self.header,
                &widths,
                &self.alignments,
                false,
            ));
            out.push('\n');
            out.push_str(&render_table_separator(&widths, &self.alignments));
            out.push('\n');
        }

        for row in &self.rows {
            out.push_str(&render_table_row(row, &widths, &self.alignments, false));
            out.push('\n');
        }

        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalloutKind {
    Note,
    Tip,
    Warning,
    Important,
    Caution,
}

impl CalloutKind {
    fn from_marker(text: &str) -> Option<Self> {
        match text {
            "[!NOTE]" => Some(Self::Note),
            "[!TIP]" => Some(Self::Tip),
            "[!WARNING]" => Some(Self::Warning),
            "[!IMPORTANT]" => Some(Self::Important),
            "[!CAUTION]" => Some(Self::Caution),
            _ => None,
        }
    }

    fn from_token(text: &str) -> Option<Self> {
        match text {
            "CATMDCALLOUTNOTE" => Some(Self::Note),
            "CATMDCALLOUTTIP" => Some(Self::Tip),
            "CATMDCALLOUTWARNING" => Some(Self::Warning),
            "CATMDCALLOUTIMPORTANT" => Some(Self::Important),
            "CATMDCALLOUTCAUTION" => Some(Self::Caution),
            _ => None,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Self::Note => "CATMDCALLOUTNOTE",
            Self::Tip => "CATMDCALLOUTTIP",
            Self::Warning => "CATMDCALLOUTWARNING",
            Self::Important => "CATMDCALLOUTIMPORTANT",
            Self::Caution => "CATMDCALLOUTCAUTION",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Tip => "TIP",
            Self::Warning => "WARNING",
            Self::Important => "IMPORTANT",
            Self::Caution => "CAUTION",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Note => "[i]",
            Self::Tip => "[+]",
            Self::Warning => "[!]",
            Self::Important => "[*]",
            Self::Caution => "[x]",
        }
    }

    fn accent_color(self) -> &'static str {
        match self {
            Self::Note => "38;5;117",
            Self::Tip => "38;5;78",
            Self::Warning => "38;5;214",
            Self::Important => "38;5;177",
            Self::Caution => "38;5;203",
        }
    }

    fn body_color(self) -> &'static str {
        match self {
            Self::Note => "38;5;153",
            Self::Tip => "38;5;120",
            Self::Warning => "38;5;223",
            Self::Important => "38;5;225",
            Self::Caution => "38;5;217",
        }
    }
}

fn current_callout(blockquote_callouts: &[Option<CalloutKind>]) -> Option<CalloutKind> {
    blockquote_callouts.last().copied().flatten()
}

fn preprocess_callouts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());

    for (index, line) in input.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }

        out.push_str(&normalize_callout_line(line));
    }

    out
}

fn normalize_callout_line(line: &str) -> String {
    let mut cursor = 0usize;
    let bytes = line.as_bytes();
    let mut saw_quote_marker = false;

    loop {
        while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
            cursor += 1;
        }

        if cursor < bytes.len() && bytes[cursor] == b'>' {
            saw_quote_marker = true;
            cursor += 1;
            if cursor < bytes.len() && bytes[cursor] == b' ' {
                cursor += 1;
            }
            continue;
        }

        break;
    }

    if !saw_quote_marker {
        return line.to_owned();
    }

    let marker = line[cursor..].trim();
    if let Some(kind) = CalloutKind::from_marker(marker) {
        let mut normalized = String::with_capacity(line.len() + 16);
        normalized.push_str(&line[..cursor]);
        normalized.push_str(kind.token());
        return normalized;
    }

    line.to_owned()
}

pub fn render_markdown(input: &str, theme_name: &str) -> String {
    let preprocessed = preprocess_callouts(input);
    let parser = Parser::new_ext(&preprocessed, markdown_options());
    let mut out = String::new();

    let mut inline = InlineStyle::default();
    let mut list_stack: Vec<ListState> = Vec::new();
    let mut link_targets: Vec<String> = Vec::new();
    let mut code_block: Option<CodeBlockBuffer> = None;
    let mut table_state: Option<TableState> = None;
    let mut blockquote_depth = 0usize;
    let mut blockquote_callouts: Vec<Option<CalloutKind>> = Vec::new();

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

        if table_state.is_some() {
            let mut finished_table = false;
            let mut rendered_table = String::new();

            {
                let table = table_state.as_mut().expect("table state is checked above");

                match event {
                    Event::Start(tag) => match tag {
                        Tag::TableHead => table.in_header = true,
                        Tag::TableRow => table.current_row.clear(),
                        Tag::TableCell => table.current_cell.clear(),
                        Tag::Link(_, destination, _) | Tag::Image(_, destination, _) => {
                            table.link_targets.push(destination.to_string());
                        }
                        _ => {}
                    },
                    Event::End(tag) => match tag {
                        Tag::TableHead => {
                            if !table.current_row.is_empty() {
                                table.finish_row();
                            }
                            table.in_header = false;
                        }
                        Tag::TableCell => table.finish_cell(),
                        Tag::TableRow => table.finish_row(),
                        Tag::Link(..) | Tag::Image(..) => {
                            if let Some(target) = table.link_targets.pop() {
                                if !table.current_cell.is_empty() {
                                    table.current_cell.push(' ');
                                }
                                table.current_cell.push('(');
                                table.current_cell.push_str(&target);
                                table.current_cell.push(')');
                            }
                        }
                        Tag::Table(_) => {
                            rendered_table = table.render();
                            finished_table = true;
                        }
                        _ => {}
                    },
                    Event::Text(text) | Event::Code(text) | Event::Html(text) => {
                        table.current_cell.push_str(&text);
                    }
                    Event::FootnoteReference(name) => {
                        table.current_cell.push('[');
                        table.current_cell.push('^');
                        table.current_cell.push_str(&name);
                        table.current_cell.push(']');
                    }
                    Event::SoftBreak | Event::HardBreak => table.current_cell.push(' '),
                    _ => {}
                }
            }

            if finished_table {
                table_state = None;

                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }

                out.push_str(&rendered_table);
                out.push('\n');
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
                Tag::BlockQuote => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    blockquote_depth += 1;
                    blockquote_callouts.push(None);
                }
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
                Tag::Table(alignments) => {
                    table_state = Some(TableState::new(alignments));
                }
                Tag::List(start) => {
                    if !list_stack.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                        ensure_blockquote_prefix(
                            &mut out,
                            blockquote_depth,
                            current_callout(&blockquote_callouts),
                        );
                    }

                    list_stack.push(ListState::new(start))
                }
                Tag::Item => {
                    ensure_blockquote_prefix(
                        &mut out,
                        blockquote_depth,
                        current_callout(&blockquote_callouts),
                    );
                    let depth = list_stack.len().saturating_sub(1);
                    out.push_str(&"  ".repeat(depth));

                    if let Some(list) = list_stack.last_mut() {
                        out.push_str(&list.marker());
                    } else {
                        out.push_str("- ");
                    }
                }
                Tag::Link(_, destination, _) | Tag::Image(_, destination, _) => {
                    link_targets.push(destination.to_string());
                }
                Tag::FootnoteDefinition(name) => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }

                    out.push_str("\x1b[2m[^");
                    out.push_str(&name);
                    out.push_str("]:\x1b[0m ");
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
                Tag::Item => {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                Tag::List(_) => {
                    list_stack.pop();

                    if list_stack.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                Tag::Link(..) | Tag::Image(..) => {
                    if let Some(target) = link_targets.pop() {
                        out.push_str("\x1b[2m (");
                        out.push_str(&target);
                        out.push_str(")\x1b[0m");
                    }
                }
                Tag::BlockQuote => {
                    blockquote_depth = blockquote_depth.saturating_sub(1);
                    blockquote_callouts.pop();

                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                Tag::FootnoteDefinition(_) => {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if blockquote_depth > 0 {
                    if let Some(kind) = CalloutKind::from_token(text.trim()) {
                        if let Some(slot) = blockquote_callouts.last_mut() {
                            *slot = Some(kind);
                        }

                        ensure_blockquote_prefix(
                            &mut out,
                            blockquote_depth,
                            current_callout(&blockquote_callouts),
                        );
                        out.push_str("\x1b[1;");
                        out.push_str(kind.accent_color());
                        out.push('m');
                        out.push_str(kind.icon());
                        out.push(' ');
                        out.push_str(kind.label());
                        out.push_str("\x1b[0m");
                        continue;
                    }
                }

                let callout = current_callout(&blockquote_callouts);
                ensure_blockquote_prefix(&mut out, blockquote_depth, callout);

                if blockquote_depth > 0 && inline.ansi_prefix().is_none() {
                    let color = callout.map(CalloutKind::body_color).unwrap_or("3;38;5;250");
                    out.push_str("\x1b[");
                    out.push_str(color);
                    out.push('m');
                    out.push_str(&text);
                    out.push_str("\x1b[0m");
                } else {
                    push_styled_text(&mut out, &text, inline);
                }
            }
            Event::Code(text) => {
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
                out.push_str("\x1b[48;5;236m\x1b[38;5;223m ");
                out.push_str(&text);
                out.push_str(" \x1b[0m");
            }
            Event::Rule => {
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
                out.push_str("\x1b[38;5;244m----------------------------------------\x1b[0m\n")
            }
            Event::SoftBreak | Event::HardBreak => {
                out.push('\n');
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
            }
            Event::TaskListMarker(checked) => {
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
                if checked {
                    out.push_str("[x] ");
                } else {
                    out.push_str("[ ] ");
                }
            }
            Event::Html(text) => {
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
                push_styled_text(&mut out, &text, inline);
            }
            Event::FootnoteReference(name) => {
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
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

fn ensure_blockquote_prefix(out: &mut String, depth: usize, callout: Option<CalloutKind>) {
    if depth == 0 {
        return;
    }

    if !out.is_empty() && !out.ends_with('\n') {
        return;
    }

    let prefix_color = callout.map(CalloutKind::accent_color).unwrap_or("38;5;244");
    out.push_str("\x1b[");
    out.push_str(prefix_color);
    out.push('m');
    for index in 0..depth {
        if index > 0 {
            out.push(' ');
        }
        out.push('>');
    }
    out.push_str(" \x1b[0m");
}

fn render_table_row(
    row: &[String],
    widths: &[usize],
    alignments: &[Alignment],
    is_separator: bool,
) -> String {
    let mut out = String::new();
    out.push('|');

    for (index, width) in widths.iter().enumerate() {
        out.push(' ');

        let alignment = alignments.get(index).copied().unwrap_or(Alignment::None);
        let value = row.get(index).map(String::as_str).unwrap_or("");

        if is_separator {
            out.push_str(&table_separator_cell(*width, alignment));
        } else {
            out.push_str(&pad_cell(value, *width, alignment));
        }

        out.push(' ');
        out.push('|');
    }

    out
}

fn render_table_separator(widths: &[usize], alignments: &[Alignment]) -> String {
    let empty: Vec<String> = Vec::new();
    render_table_row(&empty, widths, alignments, true)
}

fn table_separator_cell(width: usize, alignment: Alignment) -> String {
    let width = width.max(3);

    match alignment {
        Alignment::Left => format!(":{}", "-".repeat(width.saturating_sub(1))),
        Alignment::Center => format!(":{}:", "-".repeat(width.saturating_sub(2).max(1))),
        Alignment::Right => format!("{}:", "-".repeat(width.saturating_sub(1))),
        Alignment::None => "-".repeat(width),
    }
}

fn pad_cell(value: &str, width: usize, alignment: Alignment) -> String {
    let len = value.chars().count();

    if len >= width {
        return value.to_owned();
    }

    let padding = width - len;

    match alignment {
        Alignment::Left | Alignment::None => format!("{value}{}", " ".repeat(padding)),
        Alignment::Right => format!("{}{}", " ".repeat(padding), value),
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            format!("{}{}{}", " ".repeat(left), value, " ".repeat(right))
        }
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

    #[test]
    fn nested_lists_render_on_separate_lines() {
        let rendered = render_markdown("- parent\n  - child one\n  - child two\n", DEFAULT_THEME);

        assert!(rendered.contains("- parent\n  - child one\n  - child two\n"));
    }

    #[test]
    fn blockquotes_are_prefixed() {
        let rendered = render_markdown("> quoted line", DEFAULT_THEME);
        assert!(rendered.contains("\x1b[38;5;244m> \x1b[0m"));
        assert!(rendered.contains("quoted line"));
    }

    #[test]
    fn callout_markers_are_rendered_with_accent_style() {
        let rendered = render_markdown("> [!TIP]\n> keep output readable", DEFAULT_THEME);

        assert!(rendered.contains("\x1b[38;5;78m> \x1b[0m\x1b[1;38;5;78m[+] TIP\x1b[0m"));
        assert!(
            rendered.contains("\x1b[38;5;78m> \x1b[0m\x1b[38;5;120mkeep output readable\x1b[0m")
        );
    }

    #[test]
    fn tables_render_with_borders_and_alignment_row() {
        let rendered = render_markdown(
            "| A | B |\n| :-- | --: |\n| left | right |\n",
            DEFAULT_THEME,
        );

        assert!(rendered.contains("| A    |     B |"));
        assert!(rendered.contains("| :--- | ----: |"));
        assert!(rendered.contains("| left | right |"));
    }
}
