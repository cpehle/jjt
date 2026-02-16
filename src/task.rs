use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
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
    pub timestamp: DateTime<Utc>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub status: Status,
    pub summary: String,
    pub priority: u8,
    pub agent: Option<String>,
    pub change: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub blocked_by: Vec<String>,
    pub links: Vec<Link>,
    pub notes: Vec<Note>,
}

impl Task {
    pub fn parse(input: &str) -> Result<Task> {
        let mut id = None;
        let mut status = None;
        let mut summary = None;
        let mut priority = 2u8;
        let mut agent = None;
        let mut change = None;
        let mut created = None;
        let mut updated = None;
        let mut blocked_by = Vec::new();
        let mut links = Vec::new();
        let mut notes = Vec::new();

        let mut lines = input.lines().peekable();

        // Parse header key-value pairs
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
                if let Some(key) = line.strip_suffix(':') {
                    match key {
                        "agent" | "change" => {}
                        _ => {}
                    }
                    continue;
                }
                continue;
            };
            match key {
                "id" => id = Some(value.to_string()),
                "status" => status = Some(value.parse()?),
                "summary" => summary = Some(value.to_string()),
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
                "created" => created = Some(value.parse()?),
                "updated" => updated = Some(value.parse()?),
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
                _ => {} // ignore unknown headers for forward compat
            }
        }

        // Parse notes (sections separated by "--- author timestamp")
        while let Some(line) = lines.next() {
            if !line.starts_with("--- ") {
                continue;
            }
            let header = &line[4..];
            let (author, timestamp_str) = header
                .split_once(' ')
                .context("invalid note header, expected 'author timestamp'")?;
            let timestamp: DateTime<Utc> = timestamp_str.parse()?;

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
                timestamp,
                body,
            });
        }

        let now = Utc::now();
        Ok(Task {
            id: id.context("missing id")?,
            status: status.unwrap_or(Status::Open),
            summary: summary.context("missing summary")?,
            priority,
            agent,
            change,
            created: created.unwrap_or(now),
            updated: updated.unwrap_or(now),
            blocked_by,
            links,
            notes,
        })
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("id: {}\n", self.id));
        out.push_str(&format!("status: {}\n", self.status));
        out.push_str(&format!("summary: {}\n", self.summary));
        out.push_str(&format!("priority: {}\n", self.priority));
        out.push_str(&format!(
            "agent: {}\n",
            self.agent.as_deref().unwrap_or("")
        ));
        out.push_str(&format!(
            "change: {}\n",
            self.change.as_deref().unwrap_or("")
        ));
        out.push_str(&format!("created: {}\n", self.created.to_rfc3339()));
        out.push_str(&format!("updated: {}\n", self.updated.to_rfc3339()));

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
            out.push_str(&format!(
                "\n--- {} {}\n",
                note.author,
                note.timestamp.to_rfc3339()
            ));
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
        let input = "\
id: jt-a1b2
status: open
summary: Refactor auth module
priority: 2
agent:
change: zxkpmory
created: 2026-02-16T10:00:00+00:00
updated: 2026-02-16T10:00:00+00:00
blocked_by: jt-c3d4 jt-e5f6
links: jt-g7h8/relates_to jt-i9j0/supersedes

--- claude 2026-02-16T10:05:00+00:00
Auth module has 3 providers,
need to handle each separately.

--- claude 2026-02-16T10:12:00+00:00
Started with OAuth provider.
";

        let task = Task::parse(input).unwrap();
        assert_eq!(task.id, "jt-a1b2");
        assert_eq!(task.status, Status::Open);
        assert_eq!(task.summary, "Refactor auth module");
        assert_eq!(task.priority, 2);
        assert!(task.agent.is_none());
        assert_eq!(task.change.as_deref(), Some("zxkpmory"));
        assert_eq!(task.blocked_by, vec!["jt-c3d4", "jt-e5f6"]);
        assert_eq!(task.links.len(), 2);
        assert_eq!(task.notes.len(), 2);
        assert_eq!(task.notes[0].author, "claude");
        assert!(task.notes[0].body.contains("3 providers"));

        // Round-trip
        let serialized = task.serialize();
        let task2 = Task::parse(&serialized).unwrap();
        assert_eq!(task2.id, task.id);
        assert_eq!(task2.status, task.status);
        assert_eq!(task2.summary, task.summary);
        assert_eq!(task2.blocked_by, task.blocked_by);
        assert_eq!(task2.notes.len(), task.notes.len());
    }

    #[test]
    fn minimal_task() {
        let input = "\
id: jt-0001
summary: Do something
";
        let task = Task::parse(input).unwrap();
        assert_eq!(task.id, "jt-0001");
        assert_eq!(task.status, Status::Open);
        assert_eq!(task.priority, 2);
        assert!(task.blocked_by.is_empty());
        assert!(task.notes.is_empty());
    }
}
