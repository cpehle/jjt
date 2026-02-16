use anyhow::{bail, Context, Result};
use std::process::Command;

pub struct Jj;

impl Jj {
    fn run(args: &[&str]) -> Result<(String, String)> {
        let out = Command::new("jj")
            .args(args)
            .output()
            .with_context(|| format!("jj not found — is it installed?"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() {
            bail!("jj {}: {}", args.join(" "), stderr.trim());
        }
        Ok((stdout, stderr))
    }

    fn stdout(args: &[&str]) -> Result<String> {
        Ok(Self::run(args)?.0.trim().to_string())
    }

    pub fn check_repo() -> Result<()> {
        Self::run(&["root"]).context("not in a jj repository")?;
        Ok(())
    }

    /// Create the jjt root bookmark.
    pub fn init_root() -> Result<()> {
        // Check if bookmark exists
        if let Ok(out) = Self::stdout(&["bookmark", "list"]) {
            for line in out.lines() {
                if line.starts_with("jjt:") || line.starts_with("jjt ") || line == "jjt" {
                    bail!("jjt bookmark already exists");
                }
            }
        }
        let (_, stderr) = Self::run(&["new", "root()", "--no-edit", "-m", "jjt root"])?;
        let id = Self::parse_change_id(&stderr)?;
        Self::run(&["bookmark", "create", "jjt", "-r", &id])?;
        Ok(())
    }

    /// Create a new commit as a child of jjt root, return its change ID.
    pub fn create_child(description: &str) -> Result<String> {
        let (_, stderr) = Self::run(&["new", "jjt", "--no-edit", "-m", description])?;
        Self::parse_change_id(&stderr)
    }

    /// Get a commit's description.
    pub fn get_description(change_id: &str) -> Result<String> {
        Self::stdout(&["log", "-r", change_id, "--no-graph", "-T", "description"])
    }

    /// Update a commit's description.
    pub fn describe(change_id: &str, description: &str) -> Result<()> {
        Self::run(&["describe", "-r", change_id, "-m", description])?;
        Ok(())
    }

    /// Abandon a commit.
    pub fn abandon(change_id: &str) -> Result<()> {
        Self::run(&["abandon", change_id])?;
        Ok(())
    }

    /// Resolve a revision spec (e.g. "@", bookmark name, change ID prefix) to a short change ID.
    pub fn resolve_change(rev: &str) -> Result<String> {
        Self::stdout(&["log", "-r", rev, "--no-graph", "-T", "change_id.short(12)"])
    }

    /// List all task commits as (change_id, description) pairs.
    pub fn list_task_records() -> Result<Vec<(String, String)>> {
        let marker = "<<JJT:END>>";
        let template = format!(
            r#""<<JJT:" ++ change_id.short(12) ++ ">>\n" ++ description ++ "\n{marker}\n""#
        );
        let (stdout, _) =
            Self::run(&["log", "-r", "children(jjt)", "--no-graph", "-T", &template])?;

        let mut results = Vec::new();
        for block in stdout.split(&format!("{marker}\n")) {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }
            let Some(nl) = block.find('\n') else {
                continue;
            };
            let header = &block[..nl];
            let description = &block[nl + 1..];

            let Some(change_id) = header
                .strip_prefix("<<JJT:")
                .and_then(|s| s.strip_suffix(">>"))
            else {
                continue;
            };

            results.push((change_id.to_string(), description.to_string()));
        }
        Ok(results)
    }

    fn parse_change_id(stderr: &str) -> Result<String> {
        for line in stderr.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Created new commit ") {
                if let Some(id) = rest.split_whitespace().next() {
                    return Ok(id.to_string());
                }
            }
        }
        bail!("could not parse change id from jj output:\n{stderr}");
    }
}
