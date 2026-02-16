use anyhow::{bail, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use std::collections::HashSet;

mod jj;
mod task;

use jj::Jj;
use task::{Link, LinkKind, Note, Status, Task};

#[derive(Parser)]
#[command(name = "jjt", about = "jj-native task tracker")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize task tracking (creates jjt bookmark)
    Init,

    /// Create a new task
    New {
        /// Task summary
        summary: String,

        /// Priority (1=highest, 5=lowest)
        #[arg(short, long, default_value_t = 2)]
        priority: u8,

        /// Link to a jj change (use @ for current change)
        #[arg(short, long)]
        change: Option<String>,
    },

    /// List tasks
    List {
        /// Only ready tasks (open, no active blockers)
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

        /// Show all tasks
        #[arg(long)]
        all: bool,
    },

    /// Show task details
    Show {
        /// Change ID (or prefix)
        id: String,
    },

    /// Claim a task
    Claim {
        id: String,

        /// Agent name (defaults to $JJT_AGENT or $USER)
        #[arg(long, env = "JJT_AGENT")]
        agent: Option<String>,
    },

    /// Mark a task as done
    Done {
        id: String,

        /// Optional closing note
        #[arg(short, long)]
        note: Option<String>,
    },

    /// Reopen a task
    Reopen { id: String },

    /// Add a blocking dependency
    Block {
        /// Task to block
        id: String,

        /// Task that blocks it (change ID)
        #[arg(long)]
        on: String,
    },

    /// Remove a blocking dependency
    Unblock {
        id: String,

        /// Blocker to remove (change ID)
        #[arg(long)]
        from: String,
    },

    /// Add a note
    Note {
        id: String,

        /// Note body
        body: String,

        /// Author (defaults to $JJT_AGENT or $USER)
        #[arg(long, env = "JJT_AGENT")]
        author: Option<String>,
    },

    /// Link two tasks
    Link {
        id: String,

        #[arg(long, group = "link_kind")]
        relates_to: Option<String>,

        #[arg(long, group = "link_kind")]
        supersedes: Option<String>,

        #[arg(long, group = "link_kind")]
        duplicates: Option<String>,
    },

    /// Abandon old done tasks
    Decay {
        /// Age threshold in days (e.g. 7d, 30d)
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

// --- Helpers ---

fn default_agent() -> Option<String> {
    std::env::var("JJT_AGENT")
        .ok()
        .or_else(|| std::env::var("USER").ok())
}

/// Load a task by change ID. Resolves to canonical form first.
fn load_task(change_id: &str) -> Result<Task> {
    let canonical = Jj::resolve_change(change_id)?;
    let desc = Jj::get_description(&canonical)?;
    Task::from_description(canonical, &desc)
}

/// Save a task back to jj by updating its commit description.
fn save_task(task: &Task) -> Result<()> {
    Jj::describe(&task.id, &task.to_description())
}

/// Resolve a change spec (could be @, a prefix, a full ID) to a change ID.
fn resolve_change(spec: &str) -> Result<String> {
    Jj::resolve_change(spec)
}

fn load_all_tasks() -> Result<Vec<Task>> {
    let records = Jj::list_task_records()?;
    let mut tasks = Vec::new();
    for (id, desc) in records {
        match Task::from_description(id, &desc) {
            Ok(task) => tasks.push(task),
            Err(e) => eprintln!("warning: skipping malformed task: {e}"),
        }
    }
    Ok(tasks)
}

// --- Commands ---

fn cmd_init(json: bool) -> Result<()> {
    Jj::check_repo()?;
    Jj::init_root()?;
    if json {
        println!(r#"{{"ok":true}}"#);
    } else {
        println!("initialized jjt (created jjt bookmark)");
    }
    Ok(())
}

fn cmd_new(summary: String, priority: u8, change: Option<String>, json: bool) -> Result<()> {
    // Resolve change spec if provided
    let change = match change {
        Some(spec) => Some(resolve_change(&spec)?),
        None => None,
    };

    let task = Task {
        id: String::new(), // placeholder, set by jj
        status: Status::Open,
        summary,
        priority,
        agent: None,
        change,
        done_at: None,
        blocked_by: vec![],
        links: vec![],
        notes: vec![],
    };
    let change_id = Jj::create_child(&task.to_description())?;
    let task = Task {
        id: change_id,
        ..task
    };

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
    let tasks = load_all_tasks()?;

    let done_ids: HashSet<&str> = tasks
        .iter()
        .filter(|t| t.status == Status::Done)
        .map(|t| t.id.as_str())
        .collect();

    let agent = default_agent();

    struct Row<'a> {
        task: &'a Task,
        is_blocked: bool,
    }

    let rows: Vec<Row> = tasks
        .iter()
        .map(|t| {
            let is_blocked = !t.blocked_by.is_empty()
                && t.blocked_by
                    .iter()
                    .any(|dep| !done_ids.contains(dep.as_str()));
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
                "{:<13} {:<8} p{}  {}{agent_str}{change_str}",
                t.id, status_str, t.priority, t.summary
            );
        }
    }
    Ok(())
}

fn cmd_show(id: &str, json: bool) -> Result<()> {
    let task = load_task(id)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&task)?);
    } else {
        println!("id: {}", task.id);
        print!("{}", task.to_description());
    }
    Ok(())
}

fn cmd_claim(id: &str, agent: Option<String>, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    let agent = agent
        .or_else(default_agent)
        .unwrap_or_else(|| "unknown".into());

    if task.status == Status::Done {
        bail!("task {} is already done", task.id);
    }
    if task.status == Status::Claimed {
        bail!(
            "task {} is already claimed by {}",
            task.id,
            task.agent.as_deref().unwrap_or("unknown")
        );
    }

    task.status = Status::Claimed;
    task.agent = Some(agent.clone());
    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} claimed by {}", task.id, agent);
    }
    Ok(())
}

fn cmd_done(id: &str, note: Option<String>, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    if task.status == Status::Done {
        bail!("task {} is already done", task.id);
    }

    task.status = Status::Done;
    task.done_at = Some(Utc::now().to_rfc3339());

    if let Some(body) = note {
        let author = task
            .agent
            .clone()
            .or_else(default_agent)
            .unwrap_or_else(|| "unknown".into());
        task.notes.push(Note {
            author,
            timestamp: Utc::now().to_rfc3339(),
            body,
        });
    }

    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} done", task.id);
    }
    Ok(())
}

fn cmd_reopen(id: &str, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    task.status = Status::Open;
    task.agent = None;
    task.done_at = None;
    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} reopened", task.id);
    }
    Ok(())
}

fn cmd_block(id: &str, on: &str, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    // Verify the blocker is a valid task
    let blocker = load_task(on)?;

    if task.id == blocker.id {
        bail!("a task cannot block itself");
    }
    if task.blocked_by.contains(&blocker.id) {
        bail!("{} is already blocked by {}", task.id, blocker.id);
    }

    task.blocked_by.push(blocker.id.clone());
    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} blocked by {}", task.id, blocker.id);
    }
    Ok(())
}

fn cmd_unblock(id: &str, from: &str, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    let blocker = load_task(from)?;

    let before = task.blocked_by.len();
    task.blocked_by.retain(|b| b != &blocker.id);
    if task.blocked_by.len() == before {
        bail!("{} is not blocked by {}", task.id, blocker.id);
    }

    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} unblocked from {}", task.id, blocker.id);
    }
    Ok(())
}

fn cmd_note(id: &str, body: &str, author: Option<String>, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    let author = author
        .or_else(|| task.agent.clone())
        .or_else(default_agent)
        .unwrap_or_else(|| "unknown".into());

    task.notes.push(Note {
        author,
        timestamp: Utc::now().to_rfc3339(),
        body: body.to_string(),
    });
    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("noted on {}", task.id);
    }
    Ok(())
}

fn cmd_link(id: &str, target: &str, kind: LinkKind, json: bool) -> Result<()> {
    let mut task = load_task(id)?;
    let target_task = load_task(target)?;

    if task
        .links
        .iter()
        .any(|l| l.target == target_task.id && l.kind == kind)
    {
        bail!(
            "{} already linked to {} as {}",
            task.id,
            target_task.id,
            kind
        );
    }

    task.links.push(Link {
        target: target_task.id.clone(),
        kind,
    });
    save_task(&task)?;

    if json {
        println!("{}", serde_json::to_string(&task)?);
    } else {
        println!("{} -> {} ({})", task.id, target_task.id, kind);
    }
    Ok(())
}

fn cmd_decay(before: &str, json: bool) -> Result<()> {
    let days: i64 = before
        .strip_suffix('d')
        .and_then(|n| n.parse().ok())
        .unwrap_or(7);
    let cutoff = Utc::now() - chrono::Duration::days(days);

    let tasks = load_all_tasks()?;
    let mut abandoned = Vec::new();

    for task in &tasks {
        if task.status == Status::Done {
            if let Some(ref done_at) = task.done_at {
                if let Ok(ts) = done_at.parse::<chrono::DateTime<Utc>>() {
                    if ts < cutoff {
                        abandoned.push(task);
                    }
                }
            }
        }
    }

    if abandoned.is_empty() {
        if json {
            println!(r#"{{"decayed":0}}"#);
        } else {
            println!("nothing to decay");
        }
        return Ok(());
    }

    let count = abandoned.len();
    for task in &abandoned {
        Jj::abandon(&task.id)?;
    }

    if json {
        println!(r#"{{"decayed":{count}}}"#);
    } else {
        println!("decayed {} tasks (jj abandon)", count);
    }
    Ok(())
}
