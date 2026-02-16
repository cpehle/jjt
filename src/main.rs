use anyhow::{bail, Result};
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use std::collections::HashSet;

mod id;
mod store;
mod task;

use store::Store;
use task::{Link, LinkKind, Note, Status, Task};

#[derive(Parser)]
#[command(name = "jjt", about = "Lightweight jj-native task tracker")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize task tracking in this repo
    Init,

    /// Create a new task
    New {
        /// Task summary
        summary: String,

        /// Priority (1=highest, 5=lowest)
        #[arg(short, long, default_value_t = 2)]
        priority: u8,

        /// Link to a jj change ID
        #[arg(short, long)]
        change: Option<String>,
    },

    /// List tasks
    List {
        /// Only tasks ready to work on (open, no active blockers)
        #[arg(long)]
        ready: bool,

        /// Only blocked tasks
        #[arg(long)]
        blocked: bool,

        /// Only tasks claimed by you
        #[arg(long)]
        mine: bool,

        /// Include done tasks
        #[arg(long)]
        done: bool,

        /// Show all tasks regardless of status
        #[arg(long)]
        all: bool,
    },

    /// Show task details
    Show {
        /// Task ID (full or prefix)
        id: String,
    },

    /// Claim a task for an agent
    Claim {
        /// Task ID
        id: String,

        /// Agent name (defaults to $JJT_AGENT or $USER)
        #[arg(long, env = "JJT_AGENT")]
        agent: Option<String>,
    },

    /// Mark a task as done
    Done {
        /// Task ID
        id: String,

        /// Optional closing note
        #[arg(short, long)]
        note: Option<String>,
    },

    /// Reopen a done or claimed task
    Reopen {
        /// Task ID
        id: String,
    },

    /// Add a blocking dependency
    Block {
        /// Task to block
        id: String,

        /// Task that blocks it
        #[arg(long)]
        on: String,
    },

    /// Remove a blocking dependency
    Unblock {
        /// Task to unblock
        id: String,

        /// Blocker to remove
        #[arg(long)]
        from: String,
    },

    /// Add a note to a task
    Note {
        /// Task ID
        id: String,

        /// Note body
        body: String,

        /// Author (defaults to $JJT_AGENT or $USER)
        #[arg(long, env = "JJT_AGENT")]
        author: Option<String>,
    },

    /// Link two tasks
    Link {
        /// Source task ID
        id: String,

        #[arg(long, group = "link_kind")]
        relates_to: Option<String>,

        #[arg(long, group = "link_kind")]
        supersedes: Option<String>,

        #[arg(long, group = "link_kind")]
        duplicates: Option<String>,
    },

    /// Compact old done tasks into a summary log
    Decay {
        /// Age threshold (e.g. 7d, 30d)
        #[arg(long, default_value = "7d")]
        before: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => cmd_init(cli.json),
        Command::New {
            summary,
            priority,
            change,
        } => cmd_new(summary, priority, change, cli.json),
        Command::List {
            ready,
            blocked,
            mine,
            done,
            all,
        } => cmd_list(ready, blocked, mine, done, all, cli.json),
        Command::Show { id } => cmd_show(&id, cli.json),
        Command::Claim { id, agent } => cmd_claim(&id, agent, cli.json),
        Command::Done { id, note } => cmd_done(&id, note, cli.json),
        Command::Reopen { id } => cmd_reopen(&id, cli.json),
        Command::Block { id, on } => cmd_block(&id, &on, cli.json),
        Command::Unblock { id, from } => cmd_unblock(&id, &from, cli.json),
        Command::Note { id, body, author } => cmd_note(&id, &body, author, cli.json),
        Command::Link {
            id,
            relates_to,
            supersedes,
            duplicates,
        } => {
            let (target, kind) = if let Some(t) = relates_to {
                (t, LinkKind::RelatesTo)
            } else if let Some(t) = supersedes {
                (t, LinkKind::Supersedes)
            } else if let Some(t) = duplicates {
                (t, LinkKind::Duplicates)
            } else {
                bail!("specify --relates-to, --supersedes, or --duplicates");
            };
            cmd_link(&id, &target, kind, cli.json)
        }
        Command::Decay { before } => cmd_decay(&before, cli.json),
    }
}

// --- Command implementations ---

fn cmd_init(json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    Store::init(&cwd)?;
    if json {
        println!(r#"{{"ok":true}}"#);
    } else {
        println!("initialized .jjt/");
    }
    Ok(())
}

fn cmd_new(summary: String, priority: u8, change: Option<String>, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.next_id()?;
    let now = Utc::now();
    let task = Task {
        id,
        status: Status::Open,
        summary,
        priority,
        agent: None,
        change,
        created: now,
        updated: now,
        blocked_by: vec![],
        links: vec![],
        notes: vec![],
    };
    store.save(&task)?;
    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{}", task.id);
    }
    Ok(())
}

fn cmd_list(
    ready: bool,
    blocked: bool,
    mine: bool,
    done: bool,
    all: bool,
    json: bool,
) -> Result<()> {
    let store = Store::open()?;
    let tasks = store.list_all()?;

    // Build set of done task IDs for computing blocked status
    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|t| t.status == Status::Done)
        .map(|t| t.id.as_str())
        .collect();

    let agent = default_agent();

    // Compute display info: is each task blocked?
    struct Row<'a> {
        task: &'a Task,
        is_blocked: bool,
    }

    let rows: Vec<Row> = tasks
        .iter()
        .map(|t| {
            let is_blocked = !t.blocked_by.is_empty()
                && t.blocked_by.iter().any(|dep| !done_ids.contains(dep.as_str()));
            Row {
                task: t,
                is_blocked,
            }
        })
        .collect();

    let filtered: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            if all {
                return true;
            }
            if ready {
                return r.task.status == Status::Open && !r.is_blocked;
            }
            if blocked {
                return r.task.status == Status::Open && r.is_blocked;
            }
            if mine {
                return r.task.status == Status::Claimed
                    && r.task.agent.as_deref() == agent.as_deref();
            }
            if done {
                return r.task.status == Status::Done;
            }
            // Default: show open and claimed (not done)
            r.task.status != Status::Done
        })
        .collect();

    if json {
        #[derive(serde::Serialize)]
        struct JsonRow<'a> {
            #[serde(flatten)]
            task: &'a Task,
            is_blocked: bool,
        }
        let json_rows: Vec<JsonRow> = filtered
            .iter()
            .map(|r| JsonRow {
                task: r.task,
                is_blocked: r.is_blocked,
            })
            .collect();
        println!("{}", serde_json::to_string(&json_rows)?);
    } else {
        if filtered.is_empty() {
            println!("no tasks");
            return Ok(());
        }
        for r in &filtered {
            let t = r.task;
            let status_str = if r.is_blocked && t.status == Status::Open {
                "blocked"
            } else {
                match t.status {
                    Status::Open => "open",
                    Status::Claimed => "claimed",
                    Status::Done => "done",
                }
            };
            let agent_str = t
                .agent
                .as_ref()
                .map(|a| format!("  [{a}]"))
                .unwrap_or_default();
            let change_str = t
                .change
                .as_ref()
                .map(|c| format!("  @{c}"))
                .unwrap_or_default();
            println!(
                "{:<9} {:<8} p{}  {}{agent_str}{change_str}",
                t.id, status_str, t.priority, t.summary
            );
        }
    }
    Ok(())
}

fn cmd_show(partial_id: &str, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let task = store.load(&id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&task)?);
    } else {
        print!("{}", task.serialize());
    }
    Ok(())
}

fn cmd_claim(partial_id: &str, agent: Option<String>, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let mut task = store.load(&id)?;

    let agent = agent.or_else(default_agent).unwrap_or_else(|| "unknown".into());

    if task.status == Status::Done {
        bail!("task {} is already done", id);
    }
    if task.status == Status::Claimed {
        if task.agent.as_deref() == Some(&agent) {
            bail!("task {} is already claimed by {}", id, agent);
        }
        bail!(
            "task {} is already claimed by {}",
            id,
            task.agent.as_deref().unwrap_or("unknown")
        );
    }

    task.status = Status::Claimed;
    task.agent = Some(agent.clone());
    task.updated = Utc::now();
    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} claimed by {}", id, agent);
    }
    Ok(())
}

fn cmd_done(partial_id: &str, note: Option<String>, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let mut task = store.load(&id)?;

    if task.status == Status::Done {
        bail!("task {} is already done", id);
    }

    task.status = Status::Done;
    task.updated = Utc::now();

    if let Some(body) = note {
        let author = task
            .agent
            .clone()
            .or_else(default_agent)
            .unwrap_or_else(|| "unknown".into());
        task.notes.push(Note {
            author,
            timestamp: Utc::now(),
            body,
        });
    }

    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} done", id);
    }
    Ok(())
}

fn cmd_reopen(partial_id: &str, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let mut task = store.load(&id)?;

    task.status = Status::Open;
    task.agent = None;
    task.updated = Utc::now();
    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} reopened", id);
    }
    Ok(())
}

fn cmd_block(partial_id: &str, on_partial: &str, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let on_id = store.resolve_id(on_partial)?;

    if id == on_id {
        bail!("a task cannot block itself");
    }

    let mut task = store.load(&id)?;
    if task.blocked_by.contains(&on_id) {
        bail!("{} is already blocked by {}", id, on_id);
    }

    task.blocked_by.push(on_id.clone());
    task.updated = Utc::now();
    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} blocked by {}", id, on_id);
    }
    Ok(())
}

fn cmd_unblock(partial_id: &str, from_partial: &str, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let from_id = store.resolve_id(from_partial)?;

    let mut task = store.load(&id)?;
    let before = task.blocked_by.len();
    task.blocked_by.retain(|b| b != &from_id);

    if task.blocked_by.len() == before {
        bail!("{} is not blocked by {}", id, from_id);
    }

    task.updated = Utc::now();
    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} unblocked from {}", id, from_id);
    }
    Ok(())
}

fn cmd_note(partial_id: &str, body: &str, author: Option<String>, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let mut task = store.load(&id)?;

    let author = author
        .or_else(|| task.agent.clone())
        .or_else(default_agent)
        .unwrap_or_else(|| "unknown".into());

    task.notes.push(Note {
        author,
        timestamp: Utc::now(),
        body: body.to_string(),
    });
    task.updated = Utc::now();
    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("noted on {}", id);
    }
    Ok(())
}

fn cmd_link(partial_id: &str, target_partial: &str, kind: LinkKind, json: bool) -> Result<()> {
    let store = Store::open()?;
    let id = store.resolve_id(partial_id)?;
    let target = store.resolve_id(target_partial)?;

    let mut task = store.load(&id)?;
    if task.links.iter().any(|l| l.target == target && l.kind == kind) {
        bail!("{} already linked to {} as {}", id, target, kind);
    }

    task.links.push(Link {
        target: target.clone(),
        kind,
    });
    task.updated = Utc::now();
    store.save(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} -> {} ({})", id, target, kind);
    }
    Ok(())
}

fn cmd_decay(before: &str, json: bool) -> Result<()> {
    let days: i64 = before
        .strip_suffix('d')
        .and_then(|n| n.parse().ok())
        .unwrap_or(7);
    let cutoff = Utc::now() - Duration::days(days);

    let store = Store::open()?;
    let tasks = store.list_all()?;

    let mut decayed = Vec::new();
    for task in &tasks {
        if task.status == Status::Done && task.updated < cutoff {
            decayed.push(task);
        }
    }

    if decayed.is_empty() {
        if json {
            println!(r#"{{"decayed":0}}"#);
        } else {
            println!("nothing to decay");
        }
        return Ok(());
    }

    // Write summary to decay log
    let mut log_entry = format!("# decayed {}\n", Utc::now().to_rfc3339());
    for task in &decayed {
        log_entry.push_str(&format!(
            "{}: {} (done {})\n",
            task.id,
            task.summary,
            task.updated.format("%Y-%m-%d")
        ));
    }
    log_entry.push('\n');
    store.append_decay_log(&log_entry)?;

    // Delete task files
    let count = decayed.len();
    for task in &decayed {
        store.delete(&task.id)?;
    }

    if json {
        println!(r#"{{"decayed":{count}}}"#);
    } else {
        println!("decayed {} tasks", count);
    }
    Ok(())
}

fn default_agent() -> Option<String> {
    std::env::var("JJT_AGENT")
        .ok()
        .or_else(|| std::env::var("USER").ok())
}
