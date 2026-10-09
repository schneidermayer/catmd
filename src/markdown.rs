use once_cell::sync::Lazy;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use std::ops::Range;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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
    fn codes(self) -> Vec<&'static str> {
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

        codes
    }

    fn ansi_prefix(self) -> Option<String> {
        let codes = self.codes();

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

const TABLE_BORDER_CODES: &str = "38;5;244";
const TABLE_HEADER_CODES: &str = "1;38;5;39";

/// Plain text is kept intact because graphemes can span Markdown parser events.
#[derive(Debug, Default)]
struct TableCell {
    plain_text: String,
    spans: Vec<TableStyleSpan>,
}

#[derive(Debug)]
struct TableStyleSpan {
    range: Range<usize>,
    codes: String,
}

#[derive(Debug, Default)]
struct TableLine {
    text: String,
    width: usize,
}

#[derive(Debug, Default)]
struct TableCellBuffer {
    cell: TableCell,
}

impl TableCellBuffer {
    fn push_styled(&mut self, text: &str, codes: &[&str]) {
        let start = self.cell.plain_text.len();
        self.cell.plain_text.push_str(&text.replace('\t', " "));
        self.cell.spans.push(TableStyleSpan {
            range: start..self.cell.plain_text.len(),
            codes: codes.join(";"),
        });
    }

    fn finish(self) -> TableCell {
        self.cell
    }
}

impl TableCell {
    fn width(&self) -> usize {
        self.plain_text
            .split('\n')
            .map(str::width)
            .max()
            .unwrap_or(0)
    }

    fn wrap(&self, width: usize) -> Vec<TableLine> {
        wrap_ranges(&self.plain_text, width)
            .into_iter()
            .map(|range| {
                let mut text = String::new();
                for span in &self.spans {
                    let start = range.start.max(span.range.start);
                    let end = range.end.min(span.range.end);
                    if start >= end {
                        continue;
                    }
                    if !span.codes.is_empty() {
                        text.push_str(&format!("\x1b[{}m", span.codes));
                    }
                    text.push_str(&self.plain_text[start..end]);
                    if !span.codes.is_empty() {
                        text.push_str("\x1b[0m");
                    }
                }
                TableLine {
                    text,
                    // Unicode widths are not always additive, even across graphemes.
                    width: self.plain_text[range].width(),
                }
            })
            .collect()
    }
}

fn wrap_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut offset = 0;
    let mut ranges = Vec::new();
    for line in text.split('\n') {
        ranges.extend(
            wrap_line_ranges(line, width)
                .into_iter()
                .map(|range| range.start + offset..range.end + offset),
        );
        offset += line.len() + 1;
    }
    ranges
}

fn wrap_line_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        if text[start..].width() <= width {
            lines.push(start..text.len());
            break;
        }

        let mut end = start;
        let mut word_break = None;
        for (offset, grapheme) in text[start..].grapheme_indices(true) {
            let next = start + offset + grapheme.len();
            if grapheme == " " && start + offset > start {
                word_break = Some(start + offset);
            }
            if text[start..next].width() > width {
                // Always make progress, including a glyph wider than the whole viewport.
                if end == start {
                    end = next;
                }
                break;
            }
            end = next;
        }
        end = word_break.filter(|&offset| offset <= end).unwrap_or(end);
        lines.push(start..end);
        start = end;
        start += text[start..]
            .graphemes(true)
            .take_while(|grapheme| *grapheme == " ")
            .map(str::len)
            .sum::<usize>();
    }
    if lines.is_empty() {
        lines.push(0..0);
    }
    lines
}

#[derive(Debug)]
struct TableState {
    alignments: Vec<Alignment>,
    header: Vec<TableCell>,
    rows: Vec<Vec<TableCell>>,
    current_row: Vec<TableCell>,
    current_cell: TableCellBuffer,
    in_header: bool,
    inline: InlineStyle,
    link_targets: Vec<String>,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: Vec::new(),
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: TableCellBuffer::default(),
            in_header: false,
            inline: InlineStyle::default(),
            link_targets: Vec::new(),
        }
    }

    fn push_text(&mut self, text: &str) {
        let mut codes = Vec::new();
        if self.in_header {
            codes.push(TABLE_HEADER_CODES);
        }
        codes.extend(self.inline.codes());

        self.current_cell.push_styled(text, &codes);
    }

    fn push_code(&mut self, text: &str) {
        self.current_cell
            .push_styled(&format!(" {text} "), &["48;5;236", "38;5;223"]);
    }

    fn push_link_target(&mut self, target: &str) {
        self.current_cell
            .push_styled(&format!(" ({target})"), &["2"]);
    }

    fn finish_cell(&mut self) {
        self.current_row
            .push(std::mem::take(&mut self.current_cell).finish());
    }

    fn finish_row(&mut self) {
        if self.in_header {
            self.header = std::mem::take(&mut self.current_row);
        } else {
            self.rows.push(std::mem::take(&mut self.current_row));
        }
    }

    fn render(&self, terminal_width: usize) -> String {
        let columns = self
            .alignments
            .len()
            .max(self.header.len())
            .max(self.rows.iter().map(|row| row.len()).max().unwrap_or(0));

        if columns == 0 {
            return String::new();
        }

        let mut widths = vec![3; columns];
        let mut minimums = vec![1; columns];

        for row in std::iter::once(&self.header).chain(&self.rows) {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(cell.width());
                minimums[index] = minimums[index].max(
                    cell.plain_text
                        .graphemes(true)
                        .map(str::width)
                        .max()
                        .unwrap_or(1),
                );
            }
        }

        let available = terminal_width.saturating_sub(3 * columns + 1);
        if minimums.iter().sum::<usize>() > available {
            return self.render_stacked(terminal_width);
        }
        // Cap the widest columns first, preserving short labels at their natural width.
        let mut budget = widths.iter().sum::<usize>();
        while budget > available {
            let index = (0..columns)
                .filter(|&index| widths[index] > minimums[index])
                .max_by_key(|&index| widths[index])
                .expect("minimum column widths fit");
            widths[index] -= 1;
            budget -= 1;
        }

        let mut out = String::new();

        out.push_str(&render_table_border(&widths, '┌', '┬', '┐'));

        if !self.header.is_empty() {
            out.push_str(&render_table_row(&self.header, &widths, &self.alignments));
            out.push_str(&render_table_border(&widths, '├', '┼', '┤'));
        }

        let multiline_rows = self.rows.iter().any(|row| {
            row.iter()
                .enumerate()
                .any(|(index, cell)| cell.plain_text.contains('\n') || cell.width() > widths[index])
        });
        for (index, row) in self.rows.iter().enumerate() {
            out.push_str(&render_table_row(row, &widths, &self.alignments));
            if multiline_rows && index + 1 < self.rows.len() {
                out.push_str(&render_table_border(&widths, '├', '┼', '┤'));
            }
        }

        out.push_str(&render_table_border(&widths, '└', '┴', '┘'));

        out
    }

    fn render_stacked(&self, width: usize) -> String {
        let mut out = String::new();
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                for value in self
                    .header
                    .get(index)
                    .into_iter()
                    .chain(std::iter::once(cell))
                {
                    for line in value.wrap(width) {
                        out.push_str(&line.text);
                        out.push('\n');
                    }
                }
                out.push('\n');
            }
        }
        if self.rows.is_empty() {
            for cell in &self.header {
                for line in cell.wrap(width) {
                    out.push_str(&line.text);
                    out.push('\n');
                }
            }
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

pub fn render_markdown(input: &str, theme_name: &str, terminal_width: usize) -> String {
    let preprocessed = preprocess_callouts(input);
    let parser = Parser::new_ext(&preprocessed, markdown_options());
    let mut out = String::new();

    let mut inline = InlineStyle::default();
    let mut list_stack: Vec<ListState> = Vec::new();
    let mut link_targets: Vec<String> = Vec::new();
    let mut in_footnote_definition = false;
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
                        Tag::TableCell => table.current_cell = TableCellBuffer::default(),
                        Tag::Strong => table.inline.strong += 1,
                        Tag::Emphasis => table.inline.emphasis += 1,
                        Tag::Strikethrough => table.inline.strikethrough += 1,
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
                        Tag::Strong => table.inline.strong = table.inline.strong.saturating_sub(1),
                        Tag::Emphasis => {
                            table.inline.emphasis = table.inline.emphasis.saturating_sub(1)
                        }
                        Tag::Strikethrough => {
                            table.inline.strikethrough =
                                table.inline.strikethrough.saturating_sub(1)
                        }
                        Tag::Link(..) | Tag::Image(..) => {
                            if let Some(target) = table.link_targets.pop() {
                                table.push_link_target(&target);
                            }
                        }
                        Tag::Table(_) => {
                            rendered_table = table.render(terminal_width);
                            finished_table = true;
                        }
                        _ => {}
                    },
                    Event::Html(text) if is_html_line_break(&text) => table.push_text("\n"),
                    Event::Text(text) | Event::Html(text) => table.push_text(&text),
                    Event::Code(text) => table.push_code(&text),
                    Event::FootnoteReference(name) => table.push_text(&format!("[^{name}]")),
                    Event::SoftBreak | Event::HardBreak => table.push_text("\n"),
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
                Tag::Paragraph if list_stack.is_empty() && !in_footnote_definition => {
                    ensure_blank_line(&mut out)
                }
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
                    in_footnote_definition = true;
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
                Tag::Paragraph => ensure_blank_line(&mut out),
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
                    in_footnote_definition = false;
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
                if is_html_line_break(&text) {
                    out.push('\n');
                }
                ensure_blockquote_prefix(
                    &mut out,
                    blockquote_depth,
                    current_callout(&blockquote_callouts),
                );
                if !is_html_line_break(&text) {
                    push_styled_text(&mut out, &text, inline);
                }
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

fn is_html_line_break(html: &str) -> bool {
    let Some(tag) = html
        .trim()
        .strip_prefix('<')
        .and_then(|tag| tag.strip_suffix('>'))
    else {
        return false;
    };
    tag.trim()
        .trim_end_matches('/')
        .trim_end()
        .eq_ignore_ascii_case("br")
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

fn render_table_border(widths: &[usize], left: char, middle: char, right: char) -> String {
    let segments: Vec<String> = widths.iter().map(|width| "─".repeat(width + 2)).collect();
    let mut separator = [0u8; 4];

    format!(
        "\x1b[{TABLE_BORDER_CODES}m{left}{}{right}\x1b[0m\n",
        segments.join(middle.encode_utf8(&mut separator))
    )
}

fn render_table_row(row: &[TableCell], widths: &[usize], alignments: &[Alignment]) -> String {
    let border = format!("\x1b[{TABLE_BORDER_CODES}m│\x1b[0m");
    let empty = TableLine::default();
    let cells: Vec<Vec<TableLine>> = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            row.get(index)
                .map(|cell| cell.wrap(*width))
                .unwrap_or_default()
        })
        .collect();
    let height = cells.iter().map(Vec::len).max().unwrap_or(1);
    let mut out = String::new();

    for line in 0..height {
        out.push_str(&border);
        for (index, width) in widths.iter().enumerate() {
            let alignment = alignments.get(index).copied().unwrap_or(Alignment::None);
            let cell = cells[index].get(line).unwrap_or(&empty);

            out.push(' ');
            out.push_str(&pad_cell(cell, *width, alignment));
            out.push(' ');
            out.push_str(&border);
        }
        out.push('\n');
    }

    out
}

fn pad_cell(cell: &TableLine, width: usize, alignment: Alignment) -> String {
    let padding = width.saturating_sub(cell.width);
    let value = &cell.text;

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

    fn render_markdown(input: &str, theme: &str) -> String {
        super::render_markdown(input, theme, 80)
    }

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
    fn paragraphs_keep_blank_line_separation() {
        let rendered = render_markdown("first paragraph\n\nsecond paragraph\n", DEFAULT_THEME);

        assert_eq!(rendered, "first paragraph\n\nsecond paragraph\n\n");
    }

    #[test]
    fn paragraph_spacing_keeps_continuation_lines_together() {
        let rendered = render_markdown(
            "first\nsoft continuation  \nhard continuation\n\nnext",
            DEFAULT_THEME,
        );

        assert_eq!(
            rendered,
            "first\nsoft continuation\nhard continuation\n\nnext\n\n"
        );
    }

    #[test]
    fn paragraphs_are_separated_from_surrounding_lists() {
        let rendered = render_markdown("before\n\n- one\n- two\n\nafter\n", DEFAULT_THEME);

        assert_eq!(rendered, "before\n\n- one\n- two\n\nafter\n\n");
    }

    #[test]
    fn loose_lists_keep_authored_paragraph_spacing() {
        let rendered = render_markdown("- first\n\n- second\n", DEFAULT_THEME);

        assert_eq!(rendered, "- first\n\n- second\n\n");
    }

    #[test]
    fn footnotes_keep_the_marker_with_the_first_paragraph() {
        let rendered = render_markdown(
            "Text[^note].\n\n[^note]: First paragraph.\n\n    Second paragraph.\n\nAfterward.\n",
            DEFAULT_THEME,
        );

        assert_eq!(
            strip_ansi(&rendered),
            "Text[^note].\n\n[^note]: First paragraph.\n\nSecond paragraph.\n\nAfterward.\n\n"
        );
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

        assert!(rendered.contains("break\n\n1. third\n2. fourth\n"));
        assert!(!rendered.contains("break\n\n  1. third"));
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

    fn strip_ansi(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut chars = input.chars();

        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for next in chars.by_ref() {
                    if next == 'm' {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }

        out
    }

    #[test]
    fn tables_render_with_box_borders_and_alignment() {
        let rendered = render_markdown(
            "| A | B |\n| :-- | --: |\n| left | right |\n",
            DEFAULT_THEME,
        );

        assert_eq!(
            strip_ansi(&rendered),
            "┌──────┬───────┐\n\
             │ A    │     B │\n\
             ├──────┼───────┤\n\
             │ left │ right │\n\
             └──────┴───────┘\n\n"
        );
    }

    #[test]
    fn table_header_is_highlighted() {
        let rendered = render_markdown("| Name |\n| --- |\n| value |\n", DEFAULT_THEME);

        assert!(rendered.contains("\x1b[1;38;5;39mName\x1b[0m"));
        assert!(!rendered.contains("\x1b[1;38;5;39mvalue"));
    }

    #[test]
    fn table_cells_keep_inline_formatting() {
        let rendered = render_markdown(
            "| A | B |\n| --- | --- |\n| **bold** | `code` |\n",
            DEFAULT_THEME,
        );

        assert!(rendered.contains("\x1b[1mbold\x1b[0m"));
        assert!(rendered.contains("\x1b[48;5;236;38;5;223m code \x1b[0m"));
        assert!(strip_ansi(&rendered).contains("│ bold │  code  │"));
    }

    #[test]
    fn table_columns_align_with_wide_characters() {
        let rendered = render_markdown(
            "| Name | Ok |\n| --- | --- |\n| 日本語 | ✅ |\n| abc | x |\n",
            DEFAULT_THEME,
        );
        let plain = strip_ansi(&rendered);

        assert!(plain.contains("│ 日本語 │ ✅  │"));
        assert!(plain.contains("│ abc    │ x   │"));
    }

    #[test]
    fn tables_render_entity_encoded_emoji_like_literal_emoji() {
        for (literal, encoded) in [
            ("❤️", "❤&#xFE0F;"),
            ("1️⃣", "1&#xFE0F;&#x20E3;"),
            ("👨‍👩‍👧‍👦", "👨&#x200D;👩&#x200D;👧&#x200D;👦"),
            ("👍🏽", "👍&#x1F3FD;"),
            ("🇨🇭", "🇨&#x1F1ED;"),
        ] {
            let markdown = format!(
                "| {literal} | Center | Right |\n\
                 | :--- | :---: | ---: |\n\
                 | {literal} | **{literal}** | {literal} |\n\
                 | aa | aa | aa |\n"
            );
            let literal_table = strip_ansi(&render_markdown(&markdown, DEFAULT_THEME));
            let encoded_table = strip_ansi(&render_markdown(
                &markdown.replace(literal, encoded),
                DEFAULT_THEME,
            ));

            assert_eq!(encoded_table, literal_table, "entity sequence: {encoded}");

            let repeated = markdown.replace(literal, &literal.repeat(12));
            let literal_wrapped = strip_ansi(&super::render_markdown(&repeated, DEFAULT_THEME, 25));
            let encoded_wrapped = strip_ansi(&super::render_markdown(
                &repeated.replace(literal, encoded),
                DEFAULT_THEME,
                25,
            ));
            assert_eq!(
                literal_wrapped, encoded_wrapped,
                "wrapped entity: {encoded}"
            );
            assert!(literal_wrapped.lines().all(|line| line.width() <= 25));
            assert_eq!(literal_wrapped.matches(literal).count(), 48);
        }
    }

    #[test]
    fn long_table_cells_wrap_with_intact_content_and_borders() {
        let target = "../evidence/very-long-directory-name/complete-report.json";
        let hash = "0123456789abcdef".repeat(4);
        let prose = "Complete installed lifecycle replay preserved all user files.";
        let markdown = format!(
            "| Work | Evidence |\n| --- | --- |\n| Lifecycle | {prose} [Receipt]({target}) |\n| Artifact | `{hash}` |\n"
        );
        for width in [20, 40, 80, 205] {
            let rendered = super::render_markdown(&markdown, DEFAULT_THEME, width);
            let plain = strip_ansi(&rendered);
            let lines: Vec<_> = plain.lines().filter(|line| !line.is_empty()).collect();
            assert!(lines.iter().all(|line| line.width() == lines[0].width()));
            assert!(lines[0].width() <= width);
            let text: String = lines
                .iter()
                .filter(|line| line.starts_with('│'))
                .flat_map(|line| line.split('│').nth(2).unwrap().chars())
                .filter(|ch| !ch.is_whitespace() && *ch != '│')
                .collect();
            assert!(text.contains(&hash));
            assert!(text.contains(target));
            assert!(text.contains(&prose.replace(' ', "")));
            if width <= 80 {
                assert_eq!(lines.iter().filter(|line| line.starts_with('├')).count(), 2);
            }
        }
    }

    #[test]
    fn wrapped_cells_keep_styles_and_align_each_line() {
        let markdown = "| L | C | R |\n| :--- | :---: | ---: |\n| **alpha beta** | *one two* | `123456789` |\n";
        let rendered = super::render_markdown(markdown, DEFAULT_THEME, 28);
        let plain = strip_ansi(&rendered);
        assert!(plain.contains("│ alpha  │  one   │  12345 │"), "{plain}");
        assert!(plain.contains("│ beta   │  two   │  6789  │"), "{plain}");
        assert!(rendered.contains("\x1b[1malpha\x1b[0m"));
        assert!(rendered.contains("\x1b[1mbeta\x1b[0m"));
        assert!(rendered.contains("\x1b[3mtwo\x1b[0m"));
        assert!(rendered.contains("\x1b[48;5;236;38;5;223m6789 \x1b[0m"));
    }

    #[test]
    fn explicit_cell_breaks_preserve_empty_lines_styles_and_alignment() {
        let markdown = "| A<br>B | Right |\n| --- | ---: |\n| **one<br /><br/>two**<BR> | a<br>bb |\n| next | end |\n";
        let rendered = render_markdown(markdown, DEFAULT_THEME);
        assert_eq!(
            strip_ansi(&rendered),
            "┌──────┬───────┐\n\
             │ A    │ Right │\n\
             │ B    │       │\n\
             ├──────┼───────┤\n\
             │ one  │     a │\n\
             │      │    bb │\n\
             │ two  │       │\n\
             │      │       │\n\
             ├──────┼───────┤\n\
             │ next │   end │\n\
             └──────┴───────┘\n\n"
        );
        assert!(rendered.contains("\x1b[1mone\x1b[0m"));
        assert!(rendered.contains("\x1b[1mtwo\x1b[0m"));
    }

    #[test]
    fn explicit_cell_breaks_combine_with_wrapping_and_stacked_layout() {
        let markdown = "| V |\n| --- |\n| alpha beta<br><br>gamma delta |\n";
        let plain = strip_ansi(&super::render_markdown(markdown, DEFAULT_THEME, 10));
        let body: Vec<_> = plain
            .lines()
            .skip(3)
            .filter(|line| line.starts_with('│'))
            .map(|line| line.trim_matches('│').trim())
            .collect();
        assert_eq!(body, ["alpha", "beta", "", "gamma", "delta"]);
        assert!(plain.lines().all(|line| line.width() <= 10));
        let stacked = strip_ansi(&super::render_markdown(
            "| V |\n| --- |\n| a<br><br>b |\n",
            DEFAULT_THEME,
            4,
        ));
        assert_eq!(stacked, "V\na\n\nb\n\n\n");
    }

    #[test]
    fn encoded_newlines_break_cells_but_code_and_escaped_tags_stay_literal() {
        let markdown = "| A | B |\n| --- | --- |\n| one&#10;two | `<br>` &lt;br&gt; |\n";
        let plain = strip_ansi(&render_markdown(markdown, DEFAULT_THEME));
        assert!(plain.contains("│ one │  <br>  <br> │"), "{plain}");
        assert!(plain.contains("│ two │             │"), "{plain}");
    }

    #[test]
    fn html_breaks_work_in_paragraphs_and_blockquotes() {
        let plain = strip_ansi(&render_markdown(
            "one<br>two<br />three\n\n> a<br/>b\n",
            DEFAULT_THEME,
        ));
        assert_eq!(plain, "one\ntwo\nthree\n\n> a\n> b\n\n");
    }

    #[test]
    fn wrapping_preserves_combining_characters_and_wide_graphemes() {
        let value = "日本語e\u{301}👨‍👩‍👧‍👦👍🏽".repeat(5);
        let markdown = format!("| Value |\n| --- |\n| {value} |\n");
        let plain = strip_ansi(&super::render_markdown(&markdown, DEFAULT_THEME, 12));
        assert!(plain.lines().all(|line| line.width() <= 12));
        let reconstructed: String = plain
            .lines()
            .skip(3)
            .filter(|line| line.starts_with('│'))
            .map(|line| line.trim_matches('│').trim())
            .collect();
        assert_eq!(reconstructed, value);
        assert_eq!(plain.matches("e\u{301}").count(), 5);
        assert_eq!(plain.matches("👨‍👩‍👧‍👦").count(), 5);

        for suffix in [" \u{301}x", " \u{fe0f}x"] {
            let value = format!("abcd{suffix}");
            let ranges = wrap_ranges(&value, 4);
            let reconstructed: String = ranges.into_iter().map(|range| &value[range]).collect();
            assert_eq!(reconstructed, value, "space is part of a grapheme");
        }
    }

    #[test]
    fn tiny_terminals_stack_fields_without_losing_text() {
        let markdown = "| A | B | C |\n| --- | --- | --- |\n| abcdef | | xyz |\n";
        for width in 1..13 {
            let plain = strip_ansi(&super::render_markdown(markdown, DEFAULT_THEME, width));
            assert!(plain.lines().all(|line| line.width() <= width));
            let compact: String = plain.chars().filter(|ch| !ch.is_whitespace()).collect();
            assert_eq!(compact, "AabcdefBCxyz");
        }
        let plain = strip_ansi(&super::render_markdown(
            "| 日本 |\n| --- |\n",
            DEFAULT_THEME,
            2,
        ));
        assert_eq!(plain.replace('\n', ""), "日本");
    }
}
