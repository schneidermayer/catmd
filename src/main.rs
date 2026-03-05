mod markdown;

use anyhow::{Context, Result};
use clap::Parser;
use std::fs::File;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const APP_NAME: &str = "catmd";

#[derive(Debug, Parser)]
#[command(
    name = APP_NAME,
    version,
    about = "cat-like output, with rich Markdown rendering for .md files"
)]
struct Cli {
    #[arg(
        short,
        long,
        help = "Disable rich Markdown rendering and print all inputs as raw text"
    )]
    plain: bool,

    #[arg(
        short = 'm',
        long = "markdown",
        conflicts_with = "plain",
        help = "Force Markdown rendering for every input, including stdin"
    )]
    force_markdown: bool,

    #[arg(
        long,
        default_value = "base16-ocean.dark",
        value_name = "THEME",
        help = "Syntect theme to use for fenced code blocks"
    )]
    theme: String,

    #[arg(value_name = "FILE", help = "Input files (`-` for stdin)")]
    files: Vec<PathBuf>,
}

#[derive(Debug)]
struct RunConfig {
    plain: bool,
    force_markdown: bool,
    theme: String,
    stdout_is_tty: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{APP_NAME}: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let config = RunConfig {
        plain: cli.plain,
        force_markdown: cli.force_markdown,
        theme: cli.theme,
        stdout_is_tty: io::stdout().is_terminal(),
    };

    let mut had_errors = false;
    let mut stdout = io::stdout().lock();

    if cli.files.is_empty() {
        if let Err(error) = process_stdin(&config, &mut stdout) {
            eprintln!("{APP_NAME}: -: {error:#}");
            had_errors = true;
        }
    } else {
        for path in &cli.files {
            let result = if path == Path::new("-") {
                process_stdin(&config, &mut stdout)
            } else {
                process_file(path, &config, &mut stdout)
            };

            if let Err(error) = result {
                eprintln!("{APP_NAME}: {}: {error:#}", path.display());
                had_errors = true;
            }
        }
    }

    stdout.flush().context("failed to flush stdout")?;

    Ok(if had_errors {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn process_file(path: &Path, config: &RunConfig, out: &mut dyn Write) -> Result<()> {
    if should_render_markdown(Some(path), config) {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read input file '{}'", path.display()))?;
        write_markdown(&bytes, &config.theme, out)
            .with_context(|| format!("failed to render markdown in '{}'", path.display()))
    } else {
        let file = File::open(path)
            .with_context(|| format!("failed to open input file '{}'", path.display()))?;
        copy_raw(file, out)
            .with_context(|| format!("failed to print raw contents of '{}'", path.display()))
    }
}

fn process_stdin(config: &RunConfig, out: &mut dyn Write) -> Result<()> {
    let mut stdin = io::stdin().lock();

    if should_render_markdown(None, config) {
        let mut bytes = Vec::new();
        stdin
            .read_to_end(&mut bytes)
            .context("failed to read stdin for markdown rendering")?;
        write_markdown(&bytes, &config.theme, out)
    } else {
        copy_raw(&mut stdin, out)
    }
}

fn copy_raw(mut reader: impl Read, out: &mut dyn Write) -> Result<()> {
    io::copy(&mut reader, out).context("failed while copying bytes")?;
    Ok(())
}

fn write_markdown(bytes: &[u8], theme: &str, out: &mut dyn Write) -> Result<()> {
    let content = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    };

    let rendered = markdown::render_markdown(&content, theme);
    out.write_all(rendered.as_bytes())
        .context("failed to write rendered markdown")
}

fn should_render_markdown(path: Option<&Path>, config: &RunConfig) -> bool {
    if config.plain {
        return false;
    }

    if config.force_markdown {
        return true;
    }

    if !config.stdout_is_tty {
        return false;
    }

    matches!(path, Some(path) if is_markdown_path(path))
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_extension_detection_is_case_insensitive() {
        assert!(is_markdown_path(Path::new("README.md")));
        assert!(is_markdown_path(Path::new("README.MARKDOWN")));
        assert!(is_markdown_path(Path::new("notes.MdOwN")));
    }

    #[test]
    fn non_markdown_extension_is_not_detected() {
        assert!(!is_markdown_path(Path::new("main.rs")));
        assert!(!is_markdown_path(Path::new("LICENSE")));
    }

    #[test]
    fn markdown_rendering_requires_tty_by_default() {
        let config = RunConfig {
            plain: false,
            force_markdown: false,
            theme: "base16-ocean.dark".to_owned(),
            stdout_is_tty: false,
        };

        assert!(!should_render_markdown(
            Some(Path::new("README.md")),
            &config
        ));
    }

    #[test]
    fn markdown_flag_forces_rendering() {
        let config = RunConfig {
            plain: false,
            force_markdown: true,
            theme: "base16-ocean.dark".to_owned(),
            stdout_is_tty: false,
        };

        assert!(should_render_markdown(None, &config));
    }

    #[test]
    fn plain_mode_disables_markdown_even_when_forced() {
        let config = RunConfig {
            plain: true,
            force_markdown: true,
            theme: "base16-ocean.dark".to_owned(),
            stdout_is_tty: true,
        };

        assert!(!should_render_markdown(
            Some(Path::new("README.md")),
            &config
        ));
    }
}
