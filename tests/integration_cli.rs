use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use unicode_width::UnicodeWidthStr;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture_path(name)).expect("fixture file should exist")
}

fn run_catmd(args: &[&str], stdin: Option<&[u8]>) -> Output {
    run_catmd_with_columns(args, stdin, None)
}

fn run_catmd_with_columns(args: &[&str], stdin: Option<&[u8]>, columns: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_catmd"));
    command
        .args(args)
        .env_remove("COLUMNS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(columns) = columns {
        command.env("COLUMNS", columns);
    }

    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().expect("failed to spawn catmd");

    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .expect("stdin should be piped")
            .write_all(input)
            .expect("failed to write stdin");
    }

    child
        .wait_with_output()
        .expect("failed to read process output")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "catmd exited with status {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn as_arg(path: &Path) -> &str {
    path.to_str().expect("fixture path should be valid UTF-8")
}

#[test]
fn non_markdown_fixture_matches_raw_bytes() {
    let fixture = fixture_path("plain.txt");
    let output = run_catmd(&[as_arg(&fixture)], None);

    assert_success(&output);
    assert_eq!(output.stdout, read_fixture("plain.txt"));
}

#[test]
fn markdown_fixture_defaults_to_raw_when_not_a_tty() {
    let fixture = fixture_path("markdown_sample.md");
    let output = run_catmd(&[as_arg(&fixture)], None);

    assert_success(&output);
    assert_eq!(output.stdout, read_fixture("markdown_sample.md"));
}

#[test]
fn plain_flag_disables_markdown_rendering() {
    let fixture = fixture_path("markdown_sample.md");
    let output = run_catmd(&["--plain", as_arg(&fixture)], None);

    assert_success(&output);
    assert_eq!(output.stdout, read_fixture("markdown_sample.md"));
}

#[test]
fn markdown_flag_renders_markdown_for_file() {
    let fixture = fixture_path("markdown_sample.md");
    let output = run_catmd(&["--markdown", as_arg(&fixture)], None);

    assert_success(&output);
    assert!(output.stdout.windows(2).any(|window| window == b"\x1b["));

    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(rendered.contains("Fixture Title"));
    assert!(rendered.contains("main"));
    assert!(rendered.contains("println!"));
    assert_ne!(output.stdout, read_fixture("markdown_sample.md"));
}

#[test]
fn markdown_flag_renders_markdown_from_stdin() {
    let input = read_fixture("markdown_sample.md");
    let output = run_catmd(&["--markdown", "-"], Some(&input));

    assert_success(&output);
    assert!(output.stdout.windows(2).any(|window| window == b"\x1b["));

    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(rendered.contains("Fixture Title"));
}

#[test]
fn multiple_files_are_emitted_in_argument_order() {
    let a = fixture_path("concat_a.txt");
    let b = fixture_path("concat_b.txt");

    let output = run_catmd(&[as_arg(&a), as_arg(&b)], None);

    assert_success(&output);

    let mut expected = read_fixture("concat_a.txt");
    expected.extend(read_fixture("concat_b.txt"));

    assert_eq!(output.stdout, expected);
}

const LONG_PROSE: &str =
    "All acceptance checks completed successfully with retained evidence available for review.";
const LONG_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const LONG_LINK: &str = "https://example.com/build/validation/windows-signed-acceptance-20261008/resume-20261009/final-qualification.md";

fn wide_table() -> String {
    format!(
        "| Check | Evidence |\n| --- | --- |\n| Signed | {LONG_PROSE} `{LONG_HASH}` [Report]({LONG_LINK}) |\n"
    )
}

fn strip_ansi(input: &[u8]) -> String {
    let text = String::from_utf8_lossy(input);
    let mut characters = text.chars().peekable();
    let mut plain = String::new();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.peek() == Some(&'[') {
            characters.next();
            for code in characters.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            plain.push(character);
        }
    }
    plain
}

fn assert_table_fits_and_preserves_content(output: &Output, width: usize) {
    assert_success(output);
    let plain = strip_ansi(&output.stdout);
    let table_lines: Vec<&str> = plain.lines().filter(|line| !line.is_empty()).collect();
    assert!(table_lines.len() > 5, "long table should wrap: {plain}");
    for line in &table_lines {
        assert!(
            UnicodeWidthStr::width(*line) <= width,
            "line exceeds {width} terminal columns: {line:?}",
        );
    }

    // Recover the evidence column across physical rows to catch content loss
    // when a long link or unbroken hash must be split at the column boundary.
    let evidence: String = table_lines
        .iter()
        .filter(|line| line.starts_with('│'))
        .filter_map(|line| line.split('│').nth(2))
        .flat_map(|cell| cell.chars().filter(|character| !character.is_whitespace()))
        .collect();
    for expected in [LONG_PROSE, LONG_HASH, LONG_LINK, "Report"] {
        let compact: String = expected
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        assert!(
            evidence.contains(&compact),
            "table lost content {expected:?}: {plain}",
        );
    }
}

#[test]
fn width_wraps_long_table_cells_and_preserves_their_contents() {
    let input = wide_table();
    for width in [40, 80] {
        let output = run_catmd(
            &["--markdown", "--width", &width.to_string(), "-"],
            Some(input.as_bytes()),
        );
        assert_table_fits_and_preserves_content(&output, width);
    }
}

#[test]
fn forced_rendering_uses_columns_when_stdout_is_not_a_tty() {
    let input = wide_table();
    let automatic =
        run_catmd_with_columns(&["--markdown", "-"], Some(input.as_bytes()), Some("44"));
    let explicit = run_catmd(
        &["--markdown", "--width", "44", "-"],
        Some(input.as_bytes()),
    );
    assert_table_fits_and_preserves_content(&automatic, 44);
    assert_success(&explicit);
    assert_eq!(automatic.stdout, explicit.stdout);
}

#[test]
fn explicit_width_takes_precedence_over_columns() {
    let input = wide_table();
    let overridden = run_catmd_with_columns(
        &["--markdown", "--width", "40", "-"],
        Some(input.as_bytes()),
        Some("120"),
    );
    let expected = run_catmd(
        &["--markdown", "--width", "40", "-"],
        Some(input.as_bytes()),
    );
    assert_table_fits_and_preserves_content(&overridden, 40);
    assert_success(&expected);
    assert_eq!(overridden.stdout, expected.stdout);
}

#[test]
fn absent_or_invalid_columns_falls_back_to_eighty_columns() {
    let input = wide_table();
    let expected = run_catmd(
        &["--markdown", "--width", "80", "-"],
        Some(input.as_bytes()),
    );
    assert_success(&expected);
    for columns in [None, Some("0"), Some("invalid"), Some("-1")] {
        let output = run_catmd_with_columns(&["--markdown", "-"], Some(input.as_bytes()), columns);
        assert_table_fits_and_preserves_content(&output, 80);
        assert_eq!(output.stdout, expected.stdout, "COLUMNS={columns:?}");
    }
}

#[test]
fn width_zero_is_rejected() {
    // Argument validation exits before stdin is read; writing would race that exit.
    let output = run_catmd(&["--markdown", "--width", "0", "-"], None);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("--width"), "unexpected error: {error}");
    assert!(error.contains("positive"), "unexpected error: {error}");
}

#[test]
fn width_does_not_change_raw_file_or_stdin_bytes() {
    let fixture = fixture_path("markdown_sample.md");
    let file_output = run_catmd(&["--width", "12", as_arg(&fixture)], None);
    assert_success(&file_output);
    assert_eq!(file_output.stdout, read_fixture("markdown_sample.md"));

    let input = wide_table();
    let stdin_output = run_catmd(&["--plain", "--width", "12", "-"], Some(input.as_bytes()));
    assert_success(&stdin_output);
    assert_eq!(stdin_output.stdout, input.as_bytes());
}
