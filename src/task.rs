use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Claimed,
    Done,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Open => write!(f, "open"),
            Status::Claimed => write!(f, "claimed"),
            Status::Done => write!(f, "done"),
        }
    }
}

impl FromStr for Status {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "open" => Ok(Status::Open),
            "claimed" => Ok(Status::Claimed),
            "done" => Ok(Status::Done),
            _ => bail!("unknown status: {s}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    RelatesTo,
    Duplicates,
    Supersedes,
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkKind::RelatesTo => write!(f, "relates_to"),
            LinkKind::Duplicates => write!(f, "duplicates"),
            LinkKind::Supersedes => write!(f, "supersedes"),
        }
    }
}

impl FromStr for LinkKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "relates_to" => Ok(LinkKind::RelatesTo),
            "duplicates" => Ok(LinkKind::Duplicates),
            "supersedes" => Ok(LinkKind::Supersedes),
            _ => bail!("unknown link kind: {s}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub target: String,
    pub kind: LinkKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub author: String,
    pub timestamp: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String, // jj change ID
    pub status: Status,
    pub summary: String,
    pub priority: u8,
    pub agent: Option<String>,
    pub change: Option<String>, // linked code change ID
    pub done_at: Option<String>,
    pub blocked_by: Vec<String>, // change IDs of blocking tasks
    pub links: Vec<Link>,
    pub notes: Vec<Note>,
}

impl Task {
    /// Parse a Task from a jj change ID and its commit description.
    pub fn from_description(change_id: String, description: &str) -> Result<Task> {
        let mut lines = description.lines().peekable();

        // First line: "jjt: <summary>"
        let first_line = lines.next().context("empty description")?;
        let summary = first_line
            .strip_prefix("jjt: ")
            .context("description doesn't start with 'jjt: '")?
            .to_string();

        let mut status = Status::Open;
        let mut priority = 2u8;
        let mut agent = None;
        let mut change = None;
        let mut done_at = None;
        let mut blocked_by = Vec::new();
        let mut links = Vec::new();
        let mut notes = Vec::new();

        // Parse key-value headers
        while let Some(&line) = lines.peek() {
            if line.starts_with("---") {
                break;
            }
            lines.next();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(": ") else {
                // Handle keys with empty values like "agent:"
                continue;
            };
            match key {
                "status" => status = value.parse()?,
                "priority" => priority = value.parse()?,
                "agent" => {
                    if !value.is_empty() {
                        agent = Some(value.to_string());
                    }
                }
                "change" => {
                    if !value.is_empty() {
                        change = Some(value.to_string());
                    }
                }
                "done_at" => {
                    if !value.is_empty() {
                        done_at = Some(value.to_string());
                    }
                }
                "blocked_by" => {
                    blocked_by = value.split_whitespace().map(String::from).collect();
                }
                "links" => {
                    for part in value.split_whitespace() {
                        let (target, kind) = part
                            .split_once('/')
                            .context("invalid link format, expected target/kind")?;
                        links.push(Link {
                            target: target.to_string(),
                            kind: kind.parse()?,
                        });
                    }
                }
                _ => {} // ignore unknown keys for forward compat
            }
        }

        // Parse notes
        while let Some(line) = lines.next() {
            if !line.starts_with("--- ") {
                continue;
            }
            let header = &line[4..];
            let (author, timestamp) = header
                .split_once(' ')
                .context("invalid note header, expected 'author timestamp'")?;

            let mut body = String::new();
            while let Some(&next_line) = lines.peek() {
                if next_line.starts_with("--- ") {
                    break;
                }
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(next_line);
                lines.next();
            }

            notes.push(Note {
                author: author.to_string(),
                timestamp: timestamp.to_string(),
                body,
            });
        }

        Ok(Task {
            id: change_id,
            status,
            summary,
            priority,
            agent,
            change,
            done_at,
            blocked_by,
            links,
            notes,
        })
    }

    /// Serialize to a jj commit description.
    pub fn to_description(&self) -> String {
        let mut out = format!("jjt: {}\n", self.summary);
        out.push_str(&format!("status: {}\n", self.status));
        out.push_str(&format!("priority: {}\n", self.priority));
        if let Some(ref agent) = self.agent {
            out.push_str(&format!("agent: {agent}\n"));
        }
        if let Some(ref change) = self.change {
            out.push_str(&format!("change: {change}\n"));
        }
        if let Some(ref done_at) = self.done_at {
            out.push_str(&format!("done_at: {done_at}\n"));
        }
        if !self.blocked_by.is_empty() {
            out.push_str(&format!("blocked_by: {}\n", self.blocked_by.join(" ")));
        }
        if !self.links.is_empty() {
            let links: Vec<String> = self
                .links
                .iter()
                .map(|l| format!("{}/{}", l.target, l.kind))
                .collect();
            out.push_str(&format!("links: {}\n", links.join(" ")));
        }

        for note in &self.notes {
            out.push_str(&format!("\n--- {} {}\n", note.author, note.timestamp));
            out.push_str(&note.body);
            if !note.body.ends_with('\n') {
                out.push('\n');
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let desc = "\
jjt: Refactor auth module
status: open
priority: 1
agent: claude
change: zxkpmory
blocked_by: abc123 def456
links: ghi789/relates_to xyz000/supersedes

--- claude 2026-02-16T10:05:00+00:00
Auth module has 3 providers,
need to handle each separately.

--- pehle 2026-02-16T10:12:00+00:00
Started with OAuth provider.
";

        let task = Task::from_description("vruxwmqv".into(), desc).unwrap();
        assert_eq!(task.id, "vruxwmqv");
        assert_eq!(task.status, Status::Open);
        assert_eq!(task.summary, "Refactor auth module");
        assert_eq!(task.priority, 1);
        assert_eq!(task.agent.as_deref(), Some("claude"));
        assert_eq!(task.change.as_deref(), Some("zxkpmory"));
        assert_eq!(task.blocked_by, vec!["abc123", "def456"]);
        assert_eq!(task.links.len(), 2);
        assert_eq!(task.notes.len(), 2);
        assert_eq!(task.notes[0].author, "claude");
        assert!(task.notes[0].body.contains("3 providers"));

        let serialized = task.to_description();
        let task2 = Task::from_description("vruxwmqv".into(), &serialized).unwrap();
        assert_eq!(task2.summary, task.summary);
        assert_eq!(task2.status, task.status);
        assert_eq!(task2.blocked_by, task.blocked_by);
        assert_eq!(task2.notes.len(), task.notes.len());
    }

    #[test]
    fn minimal() {
        let desc = "jjt: Do something\nstatus: open\npriority: 2\n";
        let task = Task::from_description("abc".into(), desc).unwrap();
        assert_eq!(task.summary, "Do something");
        assert_eq!(task.status, Status::Open);
        assert!(task.blocked_by.is_empty());
        assert!(task.notes.is_empty());
    }

    #[test]
    fn done_with_timestamp() {
        let desc = "jjt: Fix bug\nstatus: done\npriority: 2\ndone_at: 2026-02-16T21:00:00+00:00\n";
        let task = Task::from_description("abc".into(), desc).unwrap();
        assert_eq!(task.status, Status::Done);
        assert!(task.done_at.is_some());
    }
}
