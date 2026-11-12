//! ai-commit generates a Conventional Commits message for staged changes using
//! the Venice-backed opencode CLI.
//!
//! The model comes from `AI_MODEL_FAST` in `$XDG_CONFIG_HOME/ai/models`
//! (written by home-manager from `nix/home/models.nix`). If that is absent
//! opencode's default model is used.

use std::env;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;

const PROMPT: &str = r#"Generate a Conventional Commits message for this diff.
Template:
<type>(<scope>): <summary>
<blank line>
<body explaining why the change was made, wrapped at 72 columns>

Rules: type is one of feat, fix, refactor, perf, docs, test, build, ci, chore, style. Scope is optional. Summary is imperative mood, lower-case, no trailing period, at most 72 characters. Add a body only when the summary alone does not explain the change, and wrap it at 72 columns. Output ONLY the commit message, with no code fences."#;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Parser)]
#[command(
    about = "Generate a Conventional Commits message for staged changes.",
    disable_version_flag = true
)]
struct Args {
    /// commit the generated message instead of printing it
    #[arg(short, long)]
    commit: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ai-commit: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<ExitCode> {
    let diff = staged_diff()?;
    // Go's bytes.TrimSpace: whitespace-only means nothing is staged.
    if diff.iter().all(u8::is_ascii_whitespace) {
        style("1", "No staged changes");
        return Ok(ExitCode::FAILURE);
    }

    let mut diff_file = tempfile::NamedTempFile::new()?;
    diff_file.write_all(&diff)?;

    let message = generate(diff_file.path())?;
    if message.is_empty() {
        bail!("empty commit message");
    }

    if !args.commit {
        println!("{message}");
        return Ok(ExitCode::SUCCESS);
    }

    style("2", &message);
    // Matches the Go behaviour: a failed `git commit` exits 1 silently.
    let committed = Command::new("git")
        .args(["commit", "-m", &message])
        .status()
        .is_ok_and(|status| status.success());
    Ok(if committed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// `git diff --cached`, with Go's exact failure message.
fn staged_diff() -> Result<Vec<u8>> {
    match Command::new("git").args(["diff", "--cached"]).output() {
        Ok(output) if output.status.success() => Ok(output.stdout),
        _ => bail!("git diff --cached failed"),
    }
}

/// Run `with-nono ai opencode run` over the staged diff and return the
/// cleaned message.
fn generate(diff_path: &Path) -> Result<String> {
    let mut command = Command::new("with-nono");
    command.args(["ai", "opencode", "run", "--auto"]);
    if let Some(model) = fast_model() {
        command.args(["-m", &model]);
    }
    command.args(["--format", "json", PROMPT, "-f"]).arg(diff_path);

    // Route through $PROXY_PORT when something is listening there.
    if let Some(proxy) = proxy_url() {
        command
            .env("http_proxy", &proxy)
            .env("https_proxy", &proxy)
            .env("no_proxy", "127.0.0.1,localhost,::1")
            .env("NO_PROXY", "127.0.0.1,localhost,::1");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::null());

    let spinner = spinner();
    let output = command.output();
    spinner.finish_and_clear();

    let output = match output {
        Ok(output) => output,
        Err(err) => bail!("opencode run failed: {err}"),
    };

    let (message, api_error) = parse_events(&output.stdout);
    if let Some(api_error) = api_error {
        bail!("opencode: {api_error}");
    }
    if !output.status.success() && message.is_empty() {
        bail!(
            "opencode run failed: exit status {}",
            output.status.code().unwrap_or(-1)
        );
    }
    Ok(message)
}

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    part: Option<Part>,
    #[serde(default)]
    error: Option<ErrorInfo>,
}

#[derive(Debug, Deserialize)]
struct Part {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct ErrorInfo {
    #[serde(default)]
    message: String,
}

/// Collect the text parts of the run and surface the last API error.
///
/// The stream is a sequence of concatenated JSON values, like Go's
/// `json.Decoder`; malformed output simply ends the stream.
fn parse_events(bytes: &[u8]) -> (String, Option<String>) {
    let mut text = String::new();
    let mut api_error = None;

    for event in serde_json::Deserializer::from_slice(bytes).into_iter::<Event>() {
        let Ok(event) = event else { break };
        match event.kind.as_str() {
            "text" => {
                if let Some(part) = event.part {
                    text.push_str(&part.text);
                }
            }
            "error" => {
                api_error = event.error.map(|error| error.message);
            }
            _ => {}
        }
    }

    (trim_message(&text), api_error)
}

/// Strip trailing whitespace, code fences and surrounding blank lines while
/// keeping the blank line that separates summary and body.
fn trim_message(raw: &str) -> String {
    let lines: Vec<&str> = raw
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t', '\r']))
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect();

    let mut start = 0;
    let mut end = lines.len();
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    lines[start..end].join("\n")
}

/// Read `AI_MODEL_FAST` from the generated `ai/models` file.
fn fast_model() -> Option<String> {
    let dir = env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var("HOME").unwrap_or_default()).join(".config"));

    let contents = fs::read_to_string(dir.join("ai").join("models")).ok()?;
    for line in contents.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "AI_MODEL_FAST" {
            continue;
        }
        let value = value.trim().trim_matches(['"', '\'']);
        return (!value.is_empty()).then(|| value.to_string());
    }
    None
}

/// `http://127.0.0.1:$PROXY_PORT` when that port accepts a connection.
fn proxy_url() -> Option<String> {
    let port = env::var("PROXY_PORT")
        .ok()
        .filter(|port| !port.is_empty())
        .unwrap_or_else(|| "20172".to_string());
    let address: SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).ok()?;
    Some(format!("http://127.0.0.1:{port}"))
}

/// Spinner on stderr while the model runs. `ProgressDrawTarget::stderr` hides
/// itself when stderr is not a terminal (piped output, hm-switch).
fn spinner() -> ProgressBar {
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner} Generating...")
            .expect("valid template")
            .tick_strings(&SPINNER_FRAMES),
    );
    progress.enable_steady_tick(Duration::from_millis(100));
    progress
}

/// Print text through a gum style, falling back to plain output without gum.
fn style(colour: &str, text: &str) {
    let shown = Command::new("gum")
        .args(["style", "--foreground", colour, text])
        .status()
        .is_ok_and(|status| status.success());
    if !shown {
        println!("{text}");
    }
}
