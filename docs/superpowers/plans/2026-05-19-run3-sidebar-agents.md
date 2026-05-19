# Run 3 — Sidebar, SQLite, Multi-Agent PTY Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a 250px left sidebar with SQLite-persisted projects, inline template selection, and multi-PTY orchestration so multiple typed agents can run simultaneously each in their own project directory.

**Architecture:** Three new focused modules (`src/db/`, `src/agents/`, `src/ui/sidebar.rs`) alongside targeted changes to `src/terminal/pty.rs` and a complete overhaul of `src/ui/app.rs`. `App` loses its single-emulator/PTY fields and delegates all agent state to `AgentPool`; the 8ms tick calls `pool.tick_all()` to drain every active PTY receiver in one pass. `app.rs` is overhauled (Task 5) before `sidebar.rs` is created (Task 6) to avoid a circular dependency: `Message` variants must exist before `sidebar.rs` can reference them.

**Tech Stack:** Rust 2021, iced 0.13 (canvas + tokio), rusqlite 0.31 (bundled), rfd 0.15 (async folder picker), uuid 1 (v4), chrono 0.4, dirs 5, portable-pty 0.8.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| **Modify** | `Cargo.toml` | Add rusqlite, rfd, uuid, chrono, dirs |
| **Create** | `src/db/mod.rs` | Db struct, Project/AgentTemplate/AgentInstance types, SQLite open/seed/CRUD |
| **Modify** | `src/terminal/pty.rs` | Extend `spawn_pty` with `cwd`, `cmd`, `args` parameters |
| **Create** | `src/agents/mod.rs` | ActiveAgent, AgentPool — spawn, focus, tick_all, resize_all |
| **Modify** | `src/ui/app.rs` | Replace single-agent fields with `Db + AgentPool + sidebar_expanded_project`; add 5 new Message variants; temporary `view()` without sidebar |
| **Create** | `src/ui/sidebar.rs` | `view_sidebar()` — pure iced widget tree, no state |
| **Modify** | `src/ui/app.rs` (second pass) | Wire `view_sidebar` into `view()`, add `use` import |
| **Modify** | `src/ui/mod.rs` | Declare `pub mod sidebar` |
| **Modify** | `src/main.rs` | Declare `mod db; mod agents;` |

---

## Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the five new crates**

Replace the `[dependencies]` section in `Cargo.toml` with:

```toml
[dependencies]
iced        = { version = "0.13", features = ["canvas", "advanced", "tokio"] }
portable-pty = "0.8"
vte         = "0.13"
rusqlite    = { version = "0.31", features = ["bundled"] }
rfd         = "0.15"
uuid        = { version = "1", features = ["v4"] }
chrono      = "0.4"
dirs        = "5"
```

- [ ] **Step 2: Verify the project still compiles**

```powershell
cargo build 2>&1 | Select-String -Pattern "^error"
```

Expected: no lines starting with `error`.

- [ ] **Step 3: Commit**

```powershell
git add Cargo.toml Cargo.lock
git commit -m "chore: add rusqlite, rfd, uuid, chrono, dirs dependencies"
```

---

## Task 2: DB module

**Files:**
- Create: `src/db/mod.rs`
- Modify: `src/main.rs`

### What this does

Opens (or creates) `~/.config/choircli/db.sqlite` via rusqlite. Creates three tables on first run. Seeds `agent_templates` with one Claude entry if it is empty. Exposes typed Rust structs and a minimal CRUD API.

- [ ] **Step 1: Create `src/db/mod.rs`**

```rust
use std::path::Path;
use rusqlite::{Connection, Result, params};

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AgentTemplate {
    pub id: String,
    pub name: String,
    pub cli_command: String,
    pub base_args: Vec<String>,
    pub default_prompt: String,
}

#[derive(Debug, Clone)]
pub struct AgentInstance {
    pub id: String,
    pub project_id: String,
    pub template_id: String,
    pub custom_name: String,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open() -> Result<Self> {
        let path = dirs::config_dir()
            .expect("cannot locate config dir")
            .join("choircli")
            .join("db.sqlite");
        std::fs::create_dir_all(path.parent().unwrap()).ok();
        Self::open_at(&path)
    }

    fn open_at(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Db { conn })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id   TEXT PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agent_templates (
                id             TEXT PRIMARY KEY,
                name           TEXT NOT NULL,
                cli_command    TEXT NOT NULL,
                base_args      TEXT NOT NULL,
                default_prompt TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS project_instances (
                id              TEXT PRIMARY KEY,
                project_id      TEXT NOT NULL REFERENCES projects(id),
                template_id     TEXT NOT NULL REFERENCES agent_templates(id),
                custom_name     TEXT NOT NULL,
                last_session_id TEXT
            );",
        )?;
        Self::seed_templates(conn)?;
        Ok(())
    }

    fn seed_templates(conn: &Connection) -> Result<()> {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_templates", [], |r| r.get(0))?;
        if count == 0 {
            let args = json_encode(&["-f", "{context_file}", "-n", "Main"]);
            conn.execute(
                "INSERT INTO agent_templates (id, name, cli_command, base_args, default_prompt)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["claude-main", "Claude Main", "claude", args, ""],
            )?;
        }
        Ok(())
    }

    pub fn list_projects(&self) -> Vec<Project> {
        let mut stmt = self.conn
            .prepare("SELECT id, path, name FROM projects ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| {
            Ok(Project { id: r.get(0)?, path: r.get(1)?, name: r.get(2)? })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn insert_project(&self, path: &Path, name: &str) -> Project {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT OR IGNORE INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
            params![id, path.to_string_lossy().as_ref(), name],
        ).unwrap();
        self.conn.query_row(
            "SELECT id, path, name FROM projects WHERE path = ?1",
            params![path.to_string_lossy().as_ref()],
            |r| Ok(Project { id: r.get(0)?, path: r.get(1)?, name: r.get(2)? }),
        ).unwrap()
    }

    pub fn list_templates(&self) -> Vec<AgentTemplate> {
        let mut stmt = self.conn
            .prepare("SELECT id, name, cli_command, base_args, default_prompt FROM agent_templates")
            .unwrap();
        stmt.query_map([], |r| {
            let args_json: String = r.get(3)?;
            Ok(AgentTemplate {
                id: r.get(0)?,
                name: r.get(1)?,
                cli_command: r.get(2)?,
                base_args: json_decode(&args_json),
                default_prompt: r.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn insert_instance(&self, project_id: &str, template_id: &str) -> AgentInstance {
        let id = uuid::Uuid::new_v4().to_string();
        let custom_name = format!("{}_{}", template_id, &id[..8]);
        self.conn.execute(
            "INSERT INTO project_instances (id, project_id, template_id, custom_name)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, project_id, template_id, custom_name],
        ).unwrap();
        AgentInstance {
            id,
            project_id: project_id.to_string(),
            template_id: template_id.to_string(),
            custom_name,
        }
    }
}

// Minimal JSON array helpers — avoids pulling in serde just for this
fn json_encode(items: &[&str]) -> String {
    let parts: Vec<String> = items.iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", parts.join(","))
}

fn json_decode(s: &str) -> Vec<String> {
    let inner = s.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() { return vec![]; }
    inner.split(',')
        .map(|tok| tok.trim().trim_matches('"')
             .replace("\\\"", "\"").replace("\\\\", "\\"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Db {
        Db::open_at(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn schema_creates_without_error() {
        let _db = mem();
    }

    #[test]
    fn list_templates_returns_seeded_claude() {
        let db = mem();
        let t = db.list_templates();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].id, "claude-main");
        assert_eq!(t[0].cli_command, "claude");
        assert_eq!(t[0].base_args, vec!["-f", "{context_file}", "-n", "Main"]);
    }

    #[test]
    fn seed_is_idempotent() {
        let db = mem();
        Db::init_schema(&db.conn).unwrap();
        assert_eq!(db.list_templates().len(), 1);
    }

    #[test]
    fn insert_and_list_project_roundtrip() {
        let db = mem();
        let p = db.insert_project(Path::new("/tmp/my-proj"), "my-proj");
        assert_eq!(p.name, "my-proj");
        let list = db.list_projects();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, p.id);
    }

    #[test]
    fn insert_project_deduplicates_by_path() {
        let db = mem();
        let p1 = db.insert_project(Path::new("/tmp/dup"), "dup");
        let p2 = db.insert_project(Path::new("/tmp/dup"), "dup");
        assert_eq!(p1.id, p2.id);
        assert_eq!(db.list_projects().len(), 1);
    }

    #[test]
    fn insert_instance_stores_and_returns() {
        let db = mem();
        let proj = db.insert_project(Path::new("/tmp/p"), "p");
        let inst = db.insert_instance(&proj.id, "claude-main");
        assert_eq!(inst.project_id, proj.id);
        assert_eq!(inst.template_id, "claude-main");
    }

    #[test]
    fn json_roundtrip() {
        let enc = json_encode(&["-f", "/tmp/file.md", "-n", "Main"]);
        let dec = json_decode(&enc);
        assert_eq!(dec, vec!["-f", "/tmp/file.md", "-n", "Main"]);
    }
}
```

- [ ] **Step 2: Declare `mod db` in `src/main.rs`**

```rust
mod terminal;
mod db;
mod ui;

use ui::app::App;

static NERD_FONT_BYTES: &[u8] =
    include_bytes!("../assets/JetBrainsMonoNerdFont-Regular.ttf");

fn main() -> iced::Result {
    iced::application("ChoirCli", App::update, App::view)
        .subscription(App::subscription)
        .font(NERD_FONT_BYTES)
        .run_with(App::new)
}
```

- [ ] **Step 3: Run DB tests**

```powershell
cargo test db:: -- --nocapture
```

Expected: 7 tests pass.

- [ ] **Step 4: Build check**

```powershell
cargo build 2>&1 | Select-String "^error"
```

Expected: no errors.

- [ ] **Step 5: Commit**

```powershell
git add src/db/mod.rs src/main.rs
git commit -m "feat: add SQLite DB layer with project/template/instance CRUD"
```

---

## Task 3: Extend `spawn_pty` signature

**Files:**
- Modify: `src/terminal/pty.rs`
- Modify: `src/ui/app.rs` (interim call-site update — fully replaced in Task 5)

- [ ] **Step 1: Replace `spawn_pty` in `src/terminal/pty.rs`**

Replace the entire `pub fn spawn_pty` function (keep `use` imports and `PtyHandle` struct unchanged):

```rust
pub fn spawn_pty(
    cols: u16,
    rows: u16,
    cwd: &std::path::Path,
    cmd: &str,
    args: &[String],
) -> (PtyHandle, mpsc::Receiver<Vec<u8>>) {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .expect("failed to open PTY");

    let mut builder = CommandBuilder::new(cmd);
    builder.cwd(cwd);
    for arg in args {
        builder.arg(arg);
    }

    let child = pair.slave.spawn_command(builder).expect("failed to spawn shell");
    let writer = pair.master.take_writer().expect("failed to get PTY writer");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut reader = pair.master.try_clone_reader().expect("failed to clone PTY reader");

    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() { break; }
                }
            }
        }
    });

    (PtyHandle { master: pair.master, writer, child }, rx)
}
```

- [ ] **Step 2: Update the ignored integration test**

Replace the `#[cfg(test)] mod tests` block in `src/terminal/pty.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore]
    fn pty_spawns_shell_and_echo_works() {
        let cwd = std::env::current_dir().unwrap();
        #[cfg(windows)]
        let (cmd, args): (&str, Vec<String>) = ("cmd.exe", vec![]);
        #[cfg(not(windows))]
        let (cmd, args): (String, Vec<String>) = (
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            vec![],
        );
        let (mut pty, rx) = spawn_pty(80, 24, &cwd, &cmd, &args);

        std::thread::sleep(Duration::from_millis(500));

        let input = if cfg!(windows) { b"echo __HELLO__\r\n".as_ref() } else { b"echo __HELLO__\n".as_ref() };
        pty.write_bytes(input).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            if let Ok(data) = rx.try_recv() {
                output.extend_from_slice(&data);
                if output.windows(9).any(|w| w == b"__HELLO__") { return; }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("PTY did not echo: {:?}", String::from_utf8_lossy(&output));
    }
}
```

- [ ] **Step 3: Update the interim call in `src/ui/app.rs`**

In `App::new`, replace the `spawn_pty` call to pass the new required arguments:

```rust
pub fn new() -> (Self, Task<Message>) {
    let cwd = std::env::current_dir().unwrap_or_default();
    #[cfg(windows)]
    let default_cmd = "cmd.exe".to_string();
    #[cfg(not(windows))]
    let default_cmd = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    let (pty, rx) = spawn_pty(COLS as u16, ROWS as u16, &cwd, &default_cmd, &[]);
    let app = App {
        emulator: TerminalEmulator::new(COLS, ROWS),
        pty,
        pty_rx: Arc::new(Mutex::new(rx)),
    };
    (app, Task::none())
}
```

Leave the rest of `app.rs` unchanged.

- [ ] **Step 4: Build check**

```powershell
cargo build 2>&1 | Select-String "^error"
```

Expected: no errors.

- [ ] **Step 5: Commit**

```powershell
git add src/terminal/pty.rs src/ui/app.rs
git commit -m "feat: extend spawn_pty with cwd/cmd/args parameters"
```

---

## Task 4: Agent pool module

**Files:**
- Create: `src/agents/mod.rs`
- Modify: `src/main.rs`

`AgentPool` manages all live `ActiveAgent` instances. `spawn()` writes a context `.md` to `temp_dir`, resolves `{context_file}` in `base_args`, and launches the agent in the project directory. `tick_all()` drains every receiver.

- [ ] **Step 1: Create `src/agents/mod.rs`**

```rust
use std::sync::{Arc, Mutex, mpsc};
use std::path::Path;
use crate::terminal::{PtyHandle, TerminalEmulator, spawn_pty};
use crate::db::{Project, AgentTemplate};

pub struct ActiveAgent {
    pub id: String,
    pub project_id: String,
    pub template_name: String,
    pub spawned_at: String,
    pub emulator: TerminalEmulator,
    pub pty: PtyHandle,
    pub pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

pub struct AgentPool {
    pub agents: Vec<ActiveAgent>,
    pub focused_id: Option<String>,
}

impl AgentPool {
    pub fn new() -> Self {
        AgentPool { agents: vec![], focused_id: None }
    }

    pub fn spawn(
        &mut self,
        project: &Project,
        template: &AgentTemplate,
        cols: u16,
        rows: u16,
    ) {
        let id = uuid::Uuid::new_v4().to_string();

        let ctx_path = std::env::temp_dir()
            .join(format!("choircli_{}.md", uuid::Uuid::new_v4()));
        let ctx_content = format!(
            "# Agent Context\n\n{}\n\n## Project\n\nPath: {}\n",
            template.default_prompt, project.path
        );
        std::fs::write(&ctx_path, ctx_content).expect("failed to write context file");
        let ctx_str = ctx_path.to_string_lossy().to_string();

        let args: Vec<String> = template.base_args.iter()
            .map(|a| a.replace("{context_file}", &ctx_str))
            .collect();

        let (pty, rx) = spawn_pty(cols, rows, Path::new(&project.path), &template.cli_command, &args);

        let agent = ActiveAgent {
            id: id.clone(),
            project_id: project.id.clone(),
            template_name: template.name.clone(),
            spawned_at: chrono::Local::now().format("%H:%M").to_string(),
            emulator: TerminalEmulator::new(cols as usize, rows as usize),
            pty,
            pty_rx: Arc::new(Mutex::new(rx)),
        };

        self.agents.push(agent);
        self.focused_id = Some(id);
    }

    pub fn focused(&self) -> Option<&ActiveAgent> {
        self.focused_id.as_ref()
            .and_then(|id| self.agents.iter().find(|a| &a.id == id))
    }

    pub fn focused_mut(&mut self) -> Option<&mut ActiveAgent> {
        self.focused_id.clone()
            .and_then(|id| self.agents.iter_mut().find(|a| a.id == id))
    }

    pub fn focus(&mut self, id: &str) {
        if self.agents.iter().any(|a| a.id == id) {
            self.focused_id = Some(id.to_string());
        }
    }

    pub fn resize_all(&mut self, cols: u16, rows: u16) {
        for agent in &mut self.agents {
            agent.emulator.resize(cols as usize, rows as usize);
            agent.pty.resize(cols, rows);
        }
    }

    pub fn tick_all(&mut self) {
        for agent in &mut self.agents {
            while let Ok(bytes) = agent.pty_rx.lock().unwrap().try_recv() {
                agent.emulator.process(&bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pool_is_empty_and_focused_returns_none() {
        let pool = AgentPool::new();
        assert!(pool.agents.is_empty());
        assert!(pool.focused_id.is_none());
        assert!(pool.focused().is_none());
    }

    #[test]
    fn focus_on_unknown_id_is_noop() {
        let mut pool = AgentPool::new();
        pool.focus("does-not-exist");
        assert!(pool.focused_id.is_none());
    }

    #[test]
    #[ignore]
    fn spawn_creates_agent_and_auto_focuses() {
        let mut pool = AgentPool::new();
        let project = crate::db::Project {
            id: "p1".to_string(),
            path: std::env::current_dir().unwrap().to_string_lossy().to_string(),
            name: "test".to_string(),
        };
        let template = crate::db::AgentTemplate {
            id: "t1".to_string(),
            name: "Shell".to_string(),
            #[cfg(windows)]
            cli_command: "cmd.exe".to_string(),
            #[cfg(not(windows))]
            cli_command: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            base_args: vec![],
            default_prompt: "".to_string(),
        };
        pool.spawn(&project, &template, 80, 24);
        assert_eq!(pool.agents.len(), 1);
        assert!(pool.focused().is_some());
    }

    #[test]
    #[ignore]
    fn focus_switches_between_two_agents() {
        let mut pool = AgentPool::new();
        let project = crate::db::Project {
            id: "p1".to_string(),
            path: std::env::current_dir().unwrap().to_string_lossy().to_string(),
            name: "test".to_string(),
        };
        let template = crate::db::AgentTemplate {
            id: "t1".to_string(),
            name: "Shell".to_string(),
            #[cfg(windows)]
            cli_command: "cmd.exe".to_string(),
            #[cfg(not(windows))]
            cli_command: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            base_args: vec![],
            default_prompt: "".to_string(),
        };
        pool.spawn(&project, &template, 80, 24);
        pool.spawn(&project, &template, 80, 24);
        let first_id = pool.agents[0].id.clone();
        pool.focus(&first_id);
        assert_eq!(pool.focused_id.as_deref(), Some(first_id.as_str()));
        let second_id = pool.agents[1].id.clone();
        pool.focus(&second_id);
        assert_eq!(pool.focused_id.as_deref(), Some(second_id.as_str()));
    }
}
```

- [ ] **Step 2: Add `mod agents` to `src/main.rs`**

```rust
mod terminal;
mod db;
mod agents;
mod ui;

use ui::app::App;

static NERD_FONT_BYTES: &[u8] =
    include_bytes!("../assets/JetBrainsMonoNerdFont-Regular.ttf");

fn main() -> iced::Result {
    iced::application("ChoirCli", App::update, App::view)
        .subscription(App::subscription)
        .font(NERD_FONT_BYTES)
        .run_with(App::new)
}
```

- [ ] **Step 3: Run agent unit tests**

```powershell
cargo test agents:: -- --nocapture
```

Expected: `new_pool_is_empty_and_focused_returns_none` and `focus_on_unknown_id_is_noop` pass. Ignored tests are skipped.

- [ ] **Step 4: Build check**

```powershell
cargo build 2>&1 | Select-String "^error"
```

Expected: no errors.

- [ ] **Step 5: Commit**

```powershell
git add src/agents/mod.rs src/main.rs
git commit -m "feat: add AgentPool with multi-PTY spawn/focus/tick/resize"
```

---

## Task 5: Overhaul `src/ui/app.rs`

**Files:**
- Modify: `src/ui/app.rs`

Replace the entire file. The old single-agent fields (`emulator`, `pty`, `pty_rx`) are removed. `Db`, `AgentPool`, `projects`, `templates`, and `sidebar_expanded_project` come in. Five new `Message` variants are added. `view()` is a temporary stub (workspace only) — the sidebar is wired in Task 6 after `sidebar.rs` exists.

**Why this order matters:** `sidebar.rs` (Task 6) references `Message` variants defined here. `app.rs` (this task) will reference `view_sidebar` defined in `sidebar.rs` — but only after `sidebar.rs` exists. The `view()` in this task deliberately omits the sidebar call to avoid the circular dependency.

- [ ] **Step 1: Replace `src/ui/app.rs`**

```rust
use std::path::PathBuf;
use iced::{Element, Task, Subscription, Length};
use iced::widget::container;

use crate::db::{Db, Project, AgentTemplate};
use crate::agents::AgentPool;
use crate::ui::terminal_widget::TerminalWidget;

pub struct App {
    pub db: Db,
    pub pool: AgentPool,
    pub projects: Vec<Project>,
    pub templates: Vec<AgentTemplate>,
    pub sidebar_expanded_project: Option<String>,
    pub terminal_cols: u16,
    pub terminal_rows: u16,
}

#[derive(Debug, Clone)]
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
    CopyToClipboard(String),
    PasteFromClipboard,
    PasteText(String),
    PickDirectory,
    DirectoryPicked(Option<PathBuf>),
    ToggleTemplateMenu(String),
    SpawnAgent { project_id: String, template_id: String },
    FocusAgent(String),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let db = Db::open().expect("failed to open database");
        let projects = db.list_projects();
        let templates = db.list_templates();
        let app = App {
            db,
            pool: AgentPool::new(),
            projects,
            templates,
            sidebar_expanded_project: None,
            terminal_cols: 80,
            terminal_rows: 24,
        };
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PtyTick => {
                self.pool.tick_all();
            }
            Message::KeyInput(bytes) => {
                if let Some(agent) = self.pool.focused_mut() {
                    let _ = agent.pty.write_bytes(&bytes);
                }
            }
            Message::WindowResized(width, height) => {
                let cw = TerminalWidget::CHAR_WIDTH;
                let ch = TerminalWidget::CHAR_HEIGHT;
                let workspace_w = (width as f32 - 250.0).max(0.0);
                let cols = (workspace_w / cw).floor() as u16;
                let rows = (height as f32 / ch).floor() as u16;
                if cols > 0 && rows > 0 {
                    self.terminal_cols = cols;
                    self.terminal_rows = rows;
                    self.pool.resize_all(cols, rows);
                }
            }
            Message::CopyToClipboard(text) => {
                return iced::clipboard::write(text).map(|_: ()| Message::PtyTick);
            }
            Message::PasteFromClipboard => {
                return iced::clipboard::read()
                    .map(|opt| Message::PasteText(opt.unwrap_or_default()));
            }
            Message::PasteText(text) => {
                if let Some(agent) = self.pool.focused_mut() {
                    let _ = agent.pty.write_bytes(text.as_bytes());
                }
            }
            Message::PickDirectory => {
                return Task::future(async {
                    let folder = rfd::AsyncFileDialog::new().pick_folder().await;
                    Message::DirectoryPicked(folder.map(|f| f.path().to_path_buf()))
                });
            }
            Message::DirectoryPicked(Some(path)) => {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                let project = self.db.insert_project(&path, &name);
                if !self.projects.iter().any(|p| p.id == project.id) {
                    self.projects.push(project);
                }
            }
            Message::DirectoryPicked(None) => {}
            Message::ToggleTemplateMenu(project_id) => {
                if self.sidebar_expanded_project.as_deref() == Some(&project_id) {
                    self.sidebar_expanded_project = None;
                } else {
                    self.sidebar_expanded_project = Some(project_id);
                }
            }
            Message::SpawnAgent { project_id, template_id } => {
                if let (Some(project), Some(template)) = (
                    self.projects.iter().find(|p| p.id == project_id).cloned(),
                    self.templates.iter().find(|t| t.id == template_id).cloned(),
                ) {
                    self.db.insert_instance(&project_id, &template_id);
                    self.pool.spawn(&project, &template, self.terminal_cols, self.terminal_rows);
                    self.sidebar_expanded_project = None;
                }
            }
            Message::FocusAgent(agent_id) => {
                self.pool.focus(&agent_id);
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // Sidebar is wired in Task 6 after sidebar.rs is created
        match self.pool.focused() {
            Some(agent) => TerminalWidget::new(&agent.emulator.screen).into(),
            None => container(iced::widget::text("Sélectionne un agent").size(14))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        use std::time::Duration;

        let poll = iced::time::every(Duration::from_millis(8))
            .map(|_| Message::PtyTick);

        let resize = iced::event::listen_with(|event, _status, _id| {
            match event {
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size.width as u32, size.height as u32))
                }
                _ => None,
            }
        });

        Subscription::batch([poll, resize])
    }
}
```

- [ ] **Step 2: Run all non-ignored tests**

```powershell
cargo test 2>&1 | Select-String -Pattern "FAILED|^error"
```

Expected: no failures.

- [ ] **Step 3: Build check**

```powershell
cargo build 2>&1 | Select-String "^error"
```

Expected: no errors.

- [ ] **Step 4: Commit**

```powershell
git add src/ui/app.rs
git commit -m "feat: overhaul App — replace single-PTY with AgentPool, add 5 new Message variants"
```

---

## Task 6: Sidebar view + final wiring

**Files:**
- Create: `src/ui/sidebar.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/app.rs` (add sidebar import + update `view()`)

`view_sidebar` is a pure function — takes slices of data, returns `Element<Message>`. Standard iced widgets only (Column, Row, Button, Text, scrollable). No canvas.

- [ ] **Step 1: Create `src/ui/sidebar.rs`**

```rust
use iced::{Element, Length};
use iced::widget::{button, column, container, row, scrollable, text};
use crate::agents::ActiveAgent;
use crate::db::{AgentTemplate, Project};
use crate::ui::app::Message;

pub fn view_sidebar<'a>(
    projects: &'a [Project],
    agents: &'a [ActiveAgent],
    templates: &'a [AgentTemplate],
    expanded_project: &'a Option<String>,
) -> Element<'a, Message> {
    let mut col = column![
        button(text("📁 Select Directory").size(13))
            .on_press(Message::PickDirectory)
            .width(Length::Fill),
    ]
    .spacing(2)
    .padding(6);

    for project in projects {
        let header = row![
            text(&project.name).size(13).width(Length::Fill),
            button(text("+").size(13))
                .on_press(Message::ToggleTemplateMenu(project.id.clone()))
                .padding([2, 6]),
        ]
        .spacing(4)
        .align_y(iced::alignment::Vertical::Center);

        col = col.push(header);

        if expanded_project.as_deref() == Some(project.id.as_str()) {
            for tmpl in templates {
                let proj_id = project.id.clone();
                let tmpl_id = tmpl.id.clone();
                col = col.push(
                    button(text(format!("  ▶ {}", tmpl.name)).size(12))
                        .on_press(Message::SpawnAgent { project_id: proj_id, template_id: tmpl_id })
                        .width(Length::Fill)
                        .padding([2, 10]),
                );
            }
        }

        for agent in agents.iter().filter(|a| a.project_id == project.id) {
            col = col.push(
                button(text(format!("🤖 {} ({})", agent.template_name, agent.spawned_at)).size(12))
                    .on_press(Message::FocusAgent(agent.id.clone()))
                    .width(Length::Fill)
                    .padding([2, 16]),
            );
        }
    }

    container(scrollable(col))
        .width(250)
        .height(Length::Fill)
        .into()
}
```

- [ ] **Step 2: Declare `pub mod sidebar` in `src/ui/mod.rs`**

```rust
pub mod app;
pub mod terminal_widget;
pub mod sidebar;

#[allow(unused_imports)]
pub use app::App;
```

- [ ] **Step 3: Wire `view_sidebar` into `src/ui/app.rs`**

Add the import at the top of `src/ui/app.rs` (after the existing `use crate::ui::terminal_widget::TerminalWidget;` line):

```rust
use crate::ui::sidebar::view_sidebar;
```

Replace the `view()` method body:

```rust
pub fn view(&self) -> Element<'_, Message> {
    let workspace: Element<Message> = match self.pool.focused() {
        Some(agent) => TerminalWidget::new(&agent.emulator.screen).into(),
        None => container(iced::widget::text("Sélectionne un agent").size(14))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    };

    iced::widget::row![
        view_sidebar(
            &self.projects,
            &self.pool.agents,
            &self.templates,
            &self.sidebar_expanded_project,
        ),
        workspace,
    ]
    .into()
}
```

- [ ] **Step 4: Build check**

```powershell
cargo build 2>&1 | Select-String "^error"
```

Expected: no errors.

- [ ] **Step 5: Run full test suite**

```powershell
cargo test 2>&1 | Select-String -Pattern "FAILED|^error"
```

Expected: no failures. DB tests, agent unit tests, and terminal widget tests all pass.

- [ ] **Step 6: Commit**

```powershell
git add src/ui/sidebar.rs src/ui/mod.rs src/ui/app.rs
git commit -m "feat: add sidebar view and wire into App layout"
```

---

## Task 7: Final verification

- [ ] **Step 1: Run the full test suite one more time**

```powershell
cargo test 2>&1
```

Expected: all non-ignored tests pass. Confirm tests present from `db::tests`, `agents::tests`, `terminal_widget::tests`.

- [ ] **Step 2: Launch the app and walk through the golden path**

```powershell
cargo run
```

Manual checklist:
- [ ] Window opens: 250px sidebar on the left, "Sélectionne un agent" centered in workspace.
- [ ] Clicking "📁 Select Directory" opens the native OS folder picker.
- [ ] After picking a folder, its name appears as a row in the sidebar.
- [ ] Restarting the app (`Ctrl+C` then `cargo run`) still shows the persisted project — loaded from SQLite.
- [ ] Clicking `[+]` next to the project expands "Claude Main" template inline.
- [ ] Clicking "Claude Main" collapses the accordion; "🤖 Claude Main (HH:MM)" appears under the project.
- [ ] Clicking the agent row focuses it; the workspace now shows the terminal.
- [ ] Keyboard input, Ctrl+C, arrow keys, and Ctrl+V work in the focused terminal.
- [ ] Spawning a second agent (from the same or a different project) appears in the sidebar; clicking it switches the workspace.
- [ ] Both agents continue running simultaneously (PTYs stay alive; switching back shows the other terminal's state).

- [ ] **Step 3: Commit any minor fixes found during verification**

```powershell
git add -p
git commit -m "fix: <describe what you fixed>"
```

---

## Acceptance Criteria Cross-Check

| Criterion from spec | Implemented in |
|---|---|
| Projects survive restart (loaded from SQLite) | Task 2 (DB) + Task 5 (`App::new` calls `db.list_projects()`) |
| Terminal runs natively in project directory (`pwd` shows correct path) | Task 3 (`spawn_pty` cwd param) + Task 4 (`AgentPool::spawn` passes `project.path`) |
| `[+]` expands inline template list; clicking template spawns agent | Task 6 (sidebar accordion) + Task 5 (`ToggleTemplateMenu`, `SpawnAgent`) |
| Clicking agent in sidebar switches workspace terminal | Task 6 (sidebar agent row) + Task 5 (`FocusAgent` + `pool.focus()`) |
| Multiple agents active simultaneously, all drained at 8ms tick | Task 4 (`tick_all`) + Task 5 (`PtyTick → pool.tick_all()`) |
