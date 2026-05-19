# Run 4 — Session Resume Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore active agents at startup using `--resume <session_name>` for templates that support it (Claude) and silent fresh-start for others; add a `[×]` close button that sends SIGTERM and removes the instance from DB.

**Architecture:** Four files change in dependency order — `db/mod.rs` first (new fields + API), then `agents/mod.rs` (spawn/restore/close + SpawnAgent compile fix), then `app.rs` (CloseAgent + restore loop), then `sidebar.rs` (`[×]` button). Each task compiles independently before the next begins.

**Tech Stack:** Rust, rusqlite (SQLite), iced 0.13, portable-pty, chrono, uuid

---

## File Map

| File | Changes |
|---|---|
| `src/db/mod.rs` | `resume_arg` on `AgentTemplate`; ALTER TABLE + seed + data migrations; `last_session_id` on `AgentInstance`; unique `custom_name`; `list_instances_with_context`; `delete_instance` |
| `src/agents/mod.rs` | `resolve_args()`; new `spawn()` signature; `restore()`; `close()`; `instance_id` on `ActiveAgent` |
| `src/ui/app.rs` | `CloseAgent` message; restore loop in `new()`; update `SpawnAgent` handler |
| `src/ui/sidebar.rs` | `[×]` button per active agent row |

---

### Task 1: DB — `resume_arg` field, migrations, seed update

**Files:**
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block in `src/db/mod.rs`:

```rust
#[test]
fn seeded_template_has_resume_arg() {
    let db = mem();
    let t = db.list_templates();
    assert_eq!(t[0].resume_arg, "--resume");
}

#[test]
fn migration_sets_resume_arg_on_existing_row() {
    let db = mem();
    db.conn.execute(
        "UPDATE agent_templates SET resume_arg = '' WHERE id = 'claude-main'",
        [],
    ).unwrap();
    Db::migrate_templates(&db.conn).unwrap();
    let t = db.list_templates();
    assert_eq!(t[0].resume_arg, "--resume");
}

#[test]
fn migration_updates_base_args_from_hardcoded_main() {
    let db = mem();
    db.conn.execute(
        "UPDATE agent_templates SET base_args = ?1 WHERE id = 'claude-main'",
        rusqlite::params![super::json_encode(&["-n", "Main"])],
    ).unwrap();
    Db::migrate_templates(&db.conn).unwrap();
    let t = db.list_templates();
    assert_eq!(t[0].base_args, vec!["-n", "{session_name}"]);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```powershell
cargo test db::tests::seeded_template_has_resume_arg 2>&1 | Select-Object -Last 10
```

Expected: compile error — `resume_arg` field does not exist on `AgentTemplate`.

- [ ] **Step 3: Add `resume_arg` to `AgentTemplate`**

In `src/db/mod.rs`, update the struct:

```rust
#[derive(Debug, Clone)]
pub struct AgentTemplate {
    pub id: String,
    pub name: String,
    pub cli_command: String,
    pub base_args: Vec<String>,
    pub default_prompt: String,
    pub resume_arg: String,
}
```

- [ ] **Step 4: Add ALTER TABLE migration in `init_schema`**

Replace `init_schema` with:

```rust
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
    // Idempotent — silently ignored if column already exists
    conn.execute_batch(
        "ALTER TABLE agent_templates ADD COLUMN resume_arg TEXT NOT NULL DEFAULT '';"
    ).ok();
    Self::seed_templates(conn)?;
    Self::migrate_templates(conn)?;
    Ok(())
}
```

- [ ] **Step 5: Update `seed_templates` to include `resume_arg`**

Replace `seed_templates`:

```rust
fn seed_templates(conn: &Connection) -> Result<()> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM agent_templates", [], |r| r.get(0))?;
    if count == 0 {
        let args = json_encode(&["-n", "{session_name}"]);
        conn.execute(
            "INSERT INTO agent_templates
             (id, name, cli_command, base_args, default_prompt, resume_arg)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["claude-main", "Claude Main", "claude", args, "", "--resume"],
        )?;
    }
    Ok(())
}
```

- [ ] **Step 6: Add `migrate_templates` function**

Add after `seed_templates`:

```rust
fn migrate_templates(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE agent_templates SET resume_arg = '--resume'
         WHERE id = 'claude-main' AND resume_arg = ''",
        [],
    )?;
    conn.execute(
        "UPDATE agent_templates SET base_args = ?1
         WHERE id = 'claude-main' AND base_args = ?2",
        params![
            json_encode(&["-n", "{session_name}"]),
            json_encode(&["-n", "Main"]),
        ],
    )?;
    Ok(())
}
```

- [ ] **Step 7: Update `list_templates` to select `resume_arg`**

Replace `list_templates`:

```rust
pub fn list_templates(&self) -> Vec<AgentTemplate> {
    let mut stmt = self.conn
        .prepare(
            "SELECT id, name, cli_command, base_args, default_prompt, resume_arg
             FROM agent_templates"
        )
        .unwrap();
    stmt.query_map([], |r| {
        let args_json: String = r.get(3)?;
        Ok(AgentTemplate {
            id: r.get(0)?,
            name: r.get(1)?,
            cli_command: r.get(2)?,
            base_args: json_decode(&args_json),
            default_prompt: r.get(4)?,
            resume_arg: r.get(5)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}
```

- [ ] **Step 8: Fix existing tests that construct `AgentTemplate` without `resume_arg`**

In `src/agents/mod.rs`, the two `#[ignore]` tests (`spawn_creates_agent_and_auto_focuses` and `focus_switches_between_two_agents`) construct `AgentTemplate` literals. Add `resume_arg: String::new()` to each:

```rust
let template = crate::db::AgentTemplate {
    id: "t1".to_string(),
    name: "Shell".to_string(),
    #[cfg(windows)]
    cli_command: "cmd.exe".to_string(),
    #[cfg(not(windows))]
    cli_command: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
    base_args: vec![],
    default_prompt: "".to_string(),
    resume_arg: String::new(),
};
```

- [ ] **Step 9: Run all tests**

```powershell
cargo test 2>&1 | Select-Object -Last 15
```

Expected: all pass including `seeded_template_has_resume_arg`, `migration_sets_resume_arg_on_existing_row`, `migration_updates_base_args_from_hardcoded_main`.

- [ ] **Step 10: Commit**

```powershell
git add src/db/mod.rs src/agents/mod.rs
git commit -m "feat(db): add resume_arg to AgentTemplate with ALTER TABLE migration"
```

---

### Task 2: DB — `AgentInstance` `last_session_id`, unique `custom_name`, new API

**Files:**
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` block in `src/db/mod.rs`:

```rust
#[test]
fn insert_instance_sets_last_session_id_to_custom_name() {
    let db = mem();
    let proj = db.insert_project(Path::new("/tmp/p"), "p");
    let tmpl = db.list_templates().into_iter().next().unwrap();
    let inst = db.insert_instance(&proj.id, &tmpl);
    assert!(inst.custom_name.starts_with("Claude Main "));
    assert_eq!(inst.last_session_id.as_deref(), Some(inst.custom_name.as_str()));
}

#[test]
fn insert_instance_generates_unique_names_with_suffix() {
    let db = mem();
    let proj = db.insert_project(Path::new("/tmp/p"), "p");
    let tmpl = db.list_templates().into_iter().next().unwrap();
    let inst1 = db.insert_instance(&proj.id, &tmpl);
    // Force a collision by inserting a row with inst1's custom_name already taken
    let inst2 = db.insert_instance(&proj.id, &tmpl);
    // They must be distinct
    assert_ne!(inst1.custom_name, inst2.custom_name);
    // If same minute, second must end with " 2"
    if inst2.custom_name.starts_with(&inst1.custom_name) {
        assert!(inst2.custom_name.ends_with(" 2"));
    }
}

#[test]
fn list_instances_with_context_returns_joined_rows() {
    let db = mem();
    let proj = db.insert_project(Path::new("/tmp/p"), "p");
    let tmpl = db.list_templates().into_iter().next().unwrap();
    db.insert_instance(&proj.id, &tmpl);
    let rows = db.list_instances_with_context();
    assert_eq!(rows.len(), 1);
    let (inst, p, t) = &rows[0];
    assert_eq!(p.id, proj.id);
    assert_eq!(t.id, "claude-main");
    assert!(!inst.id.is_empty());
    assert!(inst.last_session_id.is_some());
}

#[test]
fn delete_instance_removes_row() {
    let db = mem();
    let proj = db.insert_project(Path::new("/tmp/p"), "p");
    let tmpl = db.list_templates().into_iter().next().unwrap();
    let inst = db.insert_instance(&proj.id, &tmpl);
    db.delete_instance(&inst.id);
    assert!(db.list_instances_with_context().is_empty());
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```powershell
cargo test db::tests::insert_instance_sets_last_session_id_to_custom_name 2>&1 | Select-Object -Last 10
```

Expected: compile error — `last_session_id` not on `AgentInstance`, `insert_instance` wrong signature.

- [ ] **Step 3: Add `last_session_id` to `AgentInstance`**

```rust
#[derive(Debug, Clone)]
pub struct AgentInstance {
    pub id: String,
    pub project_id: String,
    pub template_id: String,
    pub custom_name: String,
    pub last_session_id: Option<String>,
}
```

- [ ] **Step 4: Add `unique_custom_name` private helper**

Add as a method on `impl Db`:

```rust
fn unique_custom_name(&self, base: &str) -> String {
    let exists = |name: &str| -> bool {
        self.conn.query_row(
            "SELECT COUNT(*) FROM project_instances WHERE custom_name = ?1",
            params![name],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0) > 0
    };
    if !exists(base) {
        return base.to_string();
    }
    let mut i = 2u32;
    loop {
        let candidate = format!("{} {}", base, i);
        if !exists(&candidate) { return candidate; }
        i += 1;
    }
}
```

- [ ] **Step 5: Replace `insert_instance`**

```rust
pub fn insert_instance(&self, project_id: &str, template: &AgentTemplate) -> AgentInstance {
    let id = uuid::Uuid::new_v4().to_string();
    let base = format!(
        "{} {}",
        template.name,
        chrono::Local::now().format("%Hh%M")
    );
    let custom_name = self.unique_custom_name(&base);
    self.conn.execute(
        "INSERT INTO project_instances
         (id, project_id, template_id, custom_name, last_session_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, project_id, template.id, custom_name, custom_name],
    ).unwrap();
    AgentInstance {
        id,
        project_id: project_id.to_string(),
        template_id: template.id.clone(),
        custom_name: custom_name.clone(),
        last_session_id: Some(custom_name),
    }
}
```

- [ ] **Step 6: Add `list_instances_with_context` and `delete_instance`**

Add after `insert_instance`:

```rust
pub fn list_instances_with_context(&self) -> Vec<(AgentInstance, Project, AgentTemplate)> {
    let mut stmt = self.conn.prepare(
        "SELECT
            pi.id, pi.project_id, pi.template_id, pi.custom_name, pi.last_session_id,
            p.id, p.path, p.name,
            at.id, at.name, at.cli_command, at.base_args, at.default_prompt, at.resume_arg
         FROM project_instances pi
         JOIN projects p ON pi.project_id = p.id
         JOIN agent_templates at ON pi.template_id = at.id"
    ).unwrap();
    stmt.query_map([], |r| {
        let inst = AgentInstance {
            id: r.get(0)?,
            project_id: r.get(1)?,
            template_id: r.get(2)?,
            custom_name: r.get(3)?,
            last_session_id: r.get(4)?,
        };
        let proj = Project { id: r.get(5)?, path: r.get(6)?, name: r.get(7)? };
        let args_json: String = r.get(11)?;
        let tmpl = AgentTemplate {
            id: r.get(8)?,
            name: r.get(9)?,
            cli_command: r.get(10)?,
            base_args: json_decode(&args_json),
            default_prompt: r.get(12)?,
            resume_arg: r.get(13)?,
        };
        Ok((inst, proj, tmpl))
    }).unwrap().filter_map(|r| r.ok()).collect()
}

pub fn delete_instance(&self, id: &str) {
    self.conn.execute(
        "DELETE FROM project_instances WHERE id = ?1",
        params![id],
    ).unwrap();
}
```

- [ ] **Step 7: Fix the existing `insert_instance_stores_and_returns` test**

Replace it with:

```rust
#[test]
fn insert_instance_stores_and_returns() {
    let db = mem();
    let proj = db.insert_project(Path::new("/tmp/p"), "p");
    let tmpl = db.list_templates().into_iter().next().unwrap();
    let inst = db.insert_instance(&proj.id, &tmpl);
    assert_eq!(inst.project_id, proj.id);
    assert_eq!(inst.template_id, "claude-main");
}
```

- [ ] **Step 8: Fix compile error in `app.rs`**

`insert_instance` now takes `&AgentTemplate` instead of `template_id: &str`. Update the `SpawnAgent` arm in `src/ui/app.rs` minimally so it compiles (the full rewrite happens in Task 3):

```rust
Message::SpawnAgent { project_id, template_id } => {
    if let (Some(project), Some(template)) = (
        self.projects.iter().find(|p| p.id == project_id).cloned(),
        self.templates.iter().find(|t| t.id == template_id).cloned(),
    ) {
        self.db.insert_instance(&project_id, &template);
        self.pool.spawn(&project, &template, self.terminal_cols, self.terminal_rows);
        self.sidebar_expanded_project = None;
    }
}
```

- [ ] **Step 9: Run all tests**

```powershell
cargo test 2>&1 | Select-Object -Last 15
```

Expected: all pass.

- [ ] **Step 10: Commit**

```powershell
git add src/db/mod.rs src/ui/app.rs
git commit -m "feat(db): unique custom_name, last_session_id, list_instances_with_context, delete_instance"
```

---

### Task 3: Agents — `resolve_args`, updated `spawn()`, `restore()`, `close()`

**Files:**
- Modify: `src/agents/mod.rs`
- Modify: `src/ui/app.rs` (update `SpawnAgent` to pass `instance_id` + `session_name`)

- [ ] **Step 1: Write failing unit tests**

Add to `#[cfg(test)]` in `src/agents/mod.rs`:

```rust
#[test]
fn resolve_args_replaces_session_name_placeholder() {
    let args = vec![
        "-n".to_string(),
        "{session_name}".to_string(),
        "--other".to_string(),
    ];
    let resolved = super::resolve_args(&args, "Claude Main 14h22");
    assert_eq!(resolved, vec!["-n", "Claude Main 14h22", "--other"]);
}

#[test]
fn resolve_args_no_placeholder_is_identity() {
    let args = vec!["--flag".to_string(), "value".to_string()];
    let resolved = super::resolve_args(&args, "any");
    assert_eq!(resolved, vec!["--flag", "value"]);
}

#[test]
fn close_on_unknown_id_returns_none() {
    let mut pool = AgentPool::new();
    assert!(pool.close("does-not-exist").is_none());
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```powershell
cargo test agents::tests::resolve_args_replaces_session_name_placeholder 2>&1 | Select-Object -Last 10
```

Expected: compile error — `resolve_args` not defined, `close` not defined.

- [ ] **Step 3: Add `resolve_args` function**

Add before the `ActiveAgent` struct in `src/agents/mod.rs`:

```rust
fn resolve_args(args: &[String], session_name: &str) -> Vec<String> {
    args.iter()
        .map(|a| a.replace("{session_name}", session_name))
        .collect()
}
```

- [ ] **Step 4: Add `instance_id` to `ActiveAgent` and rewrite `spawn()`**

Replace the `ActiveAgent` struct:

```rust
pub struct ActiveAgent {
    pub id: String,
    pub instance_id: String,
    pub project_id: String,
    pub template_name: String,
    pub spawned_at: String,
    pub emulator: TerminalEmulator,
    pub pty: PtyHandle,
    pub pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}
```

Replace `spawn()`:

```rust
pub fn spawn(
    &mut self,
    project: &Project,
    template: &AgentTemplate,
    instance_id: &str,
    session_name: &str,
    cols: u16,
    rows: u16,
) {
    let id = uuid::Uuid::new_v4().to_string();
    let args = resolve_args(&template.base_args, session_name);
    let (pty, rx) = spawn_pty(cols, rows, Path::new(&project.path), &template.cli_command, &args);
    let agent = ActiveAgent {
        id: id.clone(),
        instance_id: instance_id.to_string(),
        project_id: project.id.clone(),
        template_name: template.name.clone(),
        spawned_at: chrono::Local::now().format("%Hh%M").to_string(),
        emulator: TerminalEmulator::new(cols as usize, rows as usize),
        pty,
        pty_rx: Arc::new(Mutex::new(rx)),
    };
    self.agents.push(agent);
    self.focused_id = Some(id);
}
```

- [ ] **Step 5: Add `restore()` method**

Add after `spawn()` in `impl AgentPool`:

```rust
pub fn restore(
    &mut self,
    project: &Project,
    template: &AgentTemplate,
    instance: &crate::db::AgentInstance,
    cols: u16,
    rows: u16,
) {
    let args: Vec<String> = if !template.resume_arg.is_empty() {
        if let Some(ref sid) = instance.last_session_id {
            vec![template.resume_arg.clone(), sid.clone()]
        } else {
            resolve_args(&template.base_args, "")
        }
    } else {
        resolve_args(&template.base_args, "")
    };

    let id = uuid::Uuid::new_v4().to_string();
    let (pty, rx) = spawn_pty(cols, rows, Path::new(&project.path), &template.cli_command, &args);
    let agent = ActiveAgent {
        id: id.clone(),
        instance_id: instance.id.clone(),
        project_id: project.id.clone(),
        template_name: template.name.clone(),
        spawned_at: chrono::Local::now().format("%Hh%M").to_string(),
        emulator: TerminalEmulator::new(cols as usize, rows as usize),
        pty,
        pty_rx: Arc::new(Mutex::new(rx)),
    };
    self.agents.push(agent);
    if self.focused_id.is_none() {
        self.focused_id = Some(id);
    }
}
```

- [ ] **Step 6: Add `close()` method**

Add after `restore()`:

```rust
pub fn close(&mut self, id: &str) -> Option<String> {
    if let Some(pos) = self.agents.iter().position(|a| a.id == id) {
        let agent = self.agents.remove(pos);
        let _ = agent.pty.child.kill();
        if self.focused_id.as_deref() == Some(id) {
            self.focused_id = None;
        }
        Some(agent.instance_id)
    } else {
        None
    }
}
```

- [ ] **Step 7: Fix `#[ignore]` tests that use old `spawn()` signature**

In `spawn_creates_agent_and_auto_focuses` and `focus_switches_between_two_agents`, update each `pool.spawn(...)` call:

```rust
pool.spawn(&project, &template, "test-instance", "Test 14h22", 80, 24);
```

- [ ] **Step 8: Update `SpawnAgent` handler in `app.rs`**

In `src/ui/app.rs`, replace the `SpawnAgent` arm:

```rust
Message::SpawnAgent { project_id, template_id } => {
    if let (Some(project), Some(template)) = (
        self.projects.iter().find(|p| p.id == project_id).cloned(),
        self.templates.iter().find(|t| t.id == template_id).cloned(),
    ) {
        let instance = self.db.insert_instance(&project_id, &template);
        let session_name = instance.custom_name.clone();
        let instance_id = instance.id.clone();
        self.pool.spawn(
            &project, &template, &instance_id, &session_name,
            self.terminal_cols, self.terminal_rows,
        );
        self.sidebar_expanded_project = None;
    }
}
```

- [ ] **Step 9: Build and run tests**

```powershell
cargo test 2>&1 | Select-Object -Last 15
```

Expected: all pass including `resolve_args_replaces_session_name_placeholder`, `resolve_args_no_placeholder_is_identity`, `close_on_unknown_id_returns_none`.

- [ ] **Step 10: Commit**

```powershell
git add src/agents/mod.rs src/ui/app.rs
git commit -m "feat(agents): add resolve_args, restore(), close(); update spawn() signature"
```

---

### Task 4: App — `CloseAgent` message + restore loop in `new()`

**Files:**
- Modify: `src/ui/app.rs`

- [ ] **Step 1: Add `CloseAgent` to the `Message` enum**

In `src/ui/app.rs`, add to the `Message` enum:

```rust
CloseAgent(String),  // agent_id
```

- [ ] **Step 2: Add `CloseAgent` handler in `update()`**

Add the arm to the `match message` block:

```rust
Message::CloseAgent(agent_id) => {
    if let Some(instance_id) = self.pool.close(&agent_id) {
        self.db.delete_instance(&instance_id);
    }
}
```

- [ ] **Step 3: Add restore loop in `App::new()`**

Replace `App::new()`:

```rust
pub fn new() -> (Self, Task<Message>) {
    let db = Db::open().expect("failed to open database");
    let projects = db.list_projects();
    let templates = db.list_templates();
    let mut pool = AgentPool::new();

    let instances = db.list_instances_with_context();
    for (instance, project, template) in &instances {
        pool.restore(project, template, instance, 80, 24);
    }

    let app = App {
        db,
        pool,
        projects,
        templates,
        sidebar_expanded_project: None,
        terminal_cols: 80,
        terminal_rows: 24,
    };
    (app, Task::none())
}
```

- [ ] **Step 4: Build**

```powershell
cargo build 2>&1 | Select-Object -Last 10
```

Expected: compiles with at most warnings. If `sidebar.rs` emits an unused `CloseAgent` warning, that is fine — it will be wired in Task 5.

- [ ] **Step 5: Run all tests**

```powershell
cargo test 2>&1 | Select-Object -Last 15
```

Expected: all pass.

- [ ] **Step 6: Commit**

```powershell
git add src/ui/app.rs
git commit -m "feat(app): add CloseAgent handler and restore loop in App::new()"
```

---

### Task 5: Sidebar — `[×]` close button

**Files:**
- Modify: `src/ui/sidebar.rs`

- [ ] **Step 1: Replace agent rows to include `[×]` button**

In `src/ui/sidebar.rs`, replace the agent rendering loop (the `for agent in agents.iter().filter(...)` block):

```rust
for agent in agents.iter().filter(|a| a.project_id == project.id) {
    let agent_row = iced::widget::row![
        button(text(format!("🤖 {} ({})", agent.template_name, agent.spawned_at)).size(12))
            .on_press(Message::FocusAgent(agent.id.clone()))
            .width(Length::Fill)
            .padding([2, 16]),
        button(text("×").size(12))
            .on_press(Message::CloseAgent(agent.id.clone()))
            .padding([2, 4]),
    ]
    .align_y(iced::alignment::Vertical::Center);
    col = col.push(agent_row);
}
```

- [ ] **Step 2: Build**

```powershell
cargo build 2>&1 | Select-Object -Last 10
```

Expected: compiles clean.

- [ ] **Step 3: Run all tests**

```powershell
cargo test 2>&1 | Select-Object -Last 15
```

Expected: all pass.

- [ ] **Step 4: Commit**

```powershell
git add src/ui/sidebar.rs
git commit -m "feat(sidebar): add [x] close button per active agent"
```

---

## Acceptance Criteria Checklist

- [ ] Cohérence après redémarrage : agents actifs réapparaissent dans la sidebar
- [ ] `--resume` fonctionnel : Claude reprend la conversation précédente
- [ ] Fresh start silencieux : template sans `resume_arg` s'ouvre à blanc
- [ ] `custom_name` unique : deux spawns à la même minute ont des noms distincts (suffixe ` 2`)
- [ ] Fermeture propre : `[×]` → SIGTERM + suppression DB + pas de restore au prochain démarrage
- [ ] Focus reset : fermer l'agent focused → workspace affiche "Sélectionne un agent"
