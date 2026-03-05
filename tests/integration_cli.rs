use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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
    let mut command = Command::new(env!("CARGO_BIN_EXE_catmd"));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

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
