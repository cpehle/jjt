use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::id;
use crate::task::Task;

pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Walk up from cwd to find .jjt directory.
    pub fn open() -> Result<Self> {
        let mut dir = std::env::current_dir()?;
        loop {
            let candidate = dir.join(".jjt");
            if candidate.is_dir() {
                return Ok(Store { root: candidate });
            }
            if !dir.pop() {
                bail!("not a jjt repository (no .jjt directory found)\nrun `jjt init` to create one");
            }
        }
    }

    /// Create .jjt directory in the given repo root.
    pub fn init(repo_root: &Path) -> Result<Self> {
        let root = repo_root.join(".jjt");
        if root.exists() {
            bail!(".jjt already exists");
        }
        fs::create_dir_all(&root)?;
        Ok(Store { root })
    }

    /// Resolve a full or partial task ID to the canonical ID.
    pub fn resolve_id(&self, partial: &str) -> Result<String> {
        let prefix = if partial.starts_with("jt-") {
            partial.to_string()
        } else {
            format!("jt-{partial}")
        };

        let matches: Vec<String> = fs::read_dir(&self.root)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let id = name.strip_suffix(".task")?;
                if id.starts_with(&prefix) {
                    Some(id.to_string())
                } else {
                    None
                }
            })
            .collect();

        match matches.len() {
            0 => bail!("no task matching '{partial}'"),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => bail!(
                "ambiguous id '{partial}', matches: {}",
                matches.join(", ")
            ),
        }
    }

    pub fn load(&self, id: &str) -> Result<Task> {
        let path = self.task_path(id);
        let content =
            fs::read_to_string(&path).with_context(|| format!("task {id} not found"))?;
        Task::parse(&content)
    }

    pub fn save(&self, task: &Task) -> Result<()> {
        fs::write(self.task_path(&task.id), task.serialize())?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        fs::remove_file(self.task_path(id))?;
        Ok(())
    }

    pub fn list_all(&self) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "task") {
                let content = fs::read_to_string(&path)?;
                match Task::parse(&content) {
                    Ok(task) => tasks.push(task),
                    Err(e) => eprintln!("warning: skipping {}: {e}", path.display()),
                }
            }
        }
        tasks.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(tasks)
    }

    pub fn next_id(&self) -> Result<String> {
        for _ in 0..100 {
            let candidate = id::generate();
            if !self.task_path(&candidate).exists() {
                return Ok(candidate);
            }
        }
        bail!("failed to generate unique id after 100 attempts")
    }

    /// Append to the decay log.
    pub fn append_decay_log(&self, entry: &str) -> Result<()> {
        use std::io::Write;
        let path = self.root.join("decay.log");
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        write!(f, "{entry}")?;
        Ok(())
    }

    fn task_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.task"))
    }
}
