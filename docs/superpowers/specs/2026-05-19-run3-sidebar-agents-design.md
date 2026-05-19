# Design : Run 3 — Sidebar Projets & Orchestration des Agents

**Date** : 2026-05-19
**Statut** : Approuvé

---

## 1. Objectif

Implémenter la sidebar latérale gauche dans `iced`, intégrer SQLite pour la persistance des projets et templates, et orchestrer le cycle de vie des terminaux PTY pour instancier plusieurs agents typés par projet.

---

## 2. Approche retenue

**Approche B — Extension modulaire** : trois nouveaux modules ciblés (`src/db/`, `src/agents/`, `src/ui/sidebar.rs`) sans suringénierie. `app.rs` reste un orchestrateur léger. Chaque module a une responsabilité claire et reste lisible indépendamment.

---

## 3. Couche DB (`src/db/mod.rs`)

### Initialisation

Au démarrage (`App::new`), on ouvre `~/.config/choircli/db.sqlite` via `rusqlite::Connection`. Le schema est créé avec `CREATE TABLE IF NOT EXISTS`. Si `agent_templates` est vide, le seed Claude est inséré :

| Champ | Valeur |
|---|---|
| `id` | `"claude-main"` |
| `name` | `"Claude Main"` |
| `cli_command` | `"claude"` |
| `base_args` | `["-f", "{context_file}", "-n", "Main"]` |
| `default_prompt` | `""` |

Le placeholder `{context_file}` est résolu au moment du spawn vers un fichier `.md` temporaire.

### Schema SQL

```sql
CREATE TABLE IF NOT EXISTS projects (
    id   TEXT PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_templates (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    cli_command     TEXT NOT NULL,
    base_args       TEXT NOT NULL,  -- JSON array
    default_prompt  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_instances (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    template_id     TEXT NOT NULL REFERENCES agent_templates(id),
    custom_name     TEXT NOT NULL,
    last_session_id TEXT
);
```

### Types Rust

```rust
pub struct Project       { pub id: String, pub path: String, pub name: String }
pub struct AgentTemplate { pub id: String, pub name: String, pub cli_command: String,
                           pub base_args: Vec<String>, pub default_prompt: String }
pub struct AgentInstance { pub id: String, pub project_id: String,
                           pub template_id: String, pub custom_name: String }
```

### API publique

```rust
pub struct Db { conn: Connection }

impl Db {
    pub fn open() -> Result<Self>
    pub fn list_projects(&self) -> Vec<Project>
    pub fn insert_project(&self, path: &Path, name: &str) -> Project
    pub fn list_templates(&self) -> Vec<AgentTemplate>
    pub fn insert_instance(&self, project_id: &str, template_id: &str) -> AgentInstance
}
```

La `Connection` n'est pas `Send` — elle reste dans le thread principal (pas de mutex nécessaire).

---

## 4. Orchestration des agents (`src/agents/mod.rs`)

### Extension de `spawn_pty`

Signature étendue pour supporter CWD, commande et arguments :

```rust
pub fn spawn_pty(cols: u16, rows: u16, cwd: &Path, cmd: &str, args: &[String])
    -> (PtyHandle, mpsc::Receiver<Vec<u8>>)
```

`CommandBuilder` reçoit `.cwd(cwd)` avant le spawn.

### Génération du contexte

Avant chaque spawn, un fichier temporaire `choircli_<uuid>.md` est créé dans `std::env::temp_dir()`. Il contient le `default_prompt` du template et le chemin du projet. La valeur `{context_file}` dans `base_args` est remplacée par ce chemin absolu. Le fichier est créé une fois au spawn.

### `ActiveAgent`

```rust
pub struct ActiveAgent {
    pub id: String,
    pub project_id: String,
    pub template_name: String,
    pub spawned_at: String,   // HH:MM pour affichage sidebar
    pub emulator: TerminalEmulator,
    pub pty: PtyHandle,
    pub pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}
```

### `AgentPool`

```rust
pub struct AgentPool {
    pub agents: Vec<ActiveAgent>,
    pub focused_id: Option<String>,
}

impl AgentPool {
    pub fn spawn(&mut self, project: &Project, template: &AgentTemplate, cols: u16, rows: u16)
    pub fn focused(&self) -> Option<&ActiveAgent>
    pub fn focused_mut(&mut self) -> Option<&mut ActiveAgent>
    pub fn focus(&mut self, id: &str)
    pub fn resize_all(&mut self, cols: u16, rows: u16)
    pub fn tick_all(&mut self)   // draine tous les pty_rx + process bytes
}
```

### Polling

Le tick 8ms appelle `pool.tick_all()` dans `update()` — tous les PTY actifs sont drainés en un seul passage.

### Resize

`WindowResized` appelle `pool.resize_all(cols, rows)` avec `cols` calculé sur `window_width - 250` (espace workspace sans sidebar).

---

## 5. Interface utilisateur

### Layout global

```rust
// App::view()
Row[
    view_sidebar(...)   // width: 250px fixe
    workspace           // width: Fill — TerminalWidget ou écran vide
]
```

### État sidebar dans `App`

```rust
pub sidebar_expanded_project: Option<String>,  // id projet dont les templates sont dépliés
```

### Structure visuelle

```
┌─────────────────────────────┐
│ [📁 Select Directory]       │
├─────────────────────────────┤
│ my-project              [+] │
│   ▼ Claude Main             │  ← expansion inline si sidebar_expanded_project == id
│     └── 🤖 Claude (10h22)  │  ← clic → FocusAgent
│ other-project           [+] │
└─────────────────────────────┘
```

- Le bouton `[+]` est **toujours visible** (pas de hover state à gérer).
- L'expansion inline affiche les templates disponibles en base sous la ligne projet.
- Les agents actifs sont affichés avec `Padding::left(16)` pour l'indentation.
- Widget standard iced (pas canvas) : `Column` de `Row`s.

### Workspace central

- `pool.focused()` = `Some(agent)` → `TerminalWidget::new(&agent.emulator.screen)`
- `pool.focused()` = `None` → `Container` avec texte "Sélectionne un agent"

---

## 6. Messages

```rust
pub enum Message {
    // existants
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
    CopyToClipboard(String),
    PasteFromClipboard,
    PasteText(String),
    // nouveaux
    PickDirectory,
    DirectoryPicked(Option<PathBuf>),
    ToggleTemplateMenu(String),                              // project_id
    SpawnAgent { project_id: String, template_id: String },
    FocusAgent(String),                                      // agent_id
}
```

### Flux spawn

```
clic [+]
  → ToggleTemplateMenu(project_id)    // déplie / referme
clic template "Claude Main"
  → SpawnAgent { project_id, template_id }
    → génère /tmp/choircli_<uuid>.md
    → pool.spawn(project, template, cols, rows)
    → db.insert_instance(project_id, template_id)
    → sidebar_expanded_project = None
```

### Flux focus

```
clic "🤖 Claude (10h22)"
  → FocusAgent(agent_id)
    → pool.focus(agent_id)
```

### Sélection dossier

`rfd::AsyncFileDialog::new().pick_folder()` → `Task::future(...)` → `Message::DirectoryPicked`. Le nom du projet = `path.file_name()`.

---

## 7. Structure des fichiers

```
src/
├── main.rs
├── db/
│   └── mod.rs               (Db, Project, AgentTemplate, AgentInstance)
├── agents/
│   └── mod.rs               (AgentPool, ActiveAgent)
├── terminal/
│   ├── mod.rs               (re-exports étendus)
│   ├── pty.rs               (spawn_pty étendu : cwd, cmd, args)
│   └── screen.rs            (inchangé)
└── ui/
    ├── mod.rs
    ├── app.rs               (App + Message étendus)
    ├── sidebar.rs           (fn view_sidebar → Element<Message>)
    └── terminal_widget.rs   (inchangé)
Cargo.toml
```

---

## 8. Nouvelles dépendances

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
rfd      = "0.15"
uuid     = { version = "1", features = ["v4"] }
```

`rusqlite` avec `bundled` évite toute dépendance système SQLite sur Windows.

---

## 9. Critères d'acceptation

- [ ] Les projets ajoutés via le sélecteur survivent au redémarrage (chargés depuis SQLite).
- [ ] Le terminal de l'agent s'exécute nativement dans le répertoire du projet (`pwd` affiche le bon chemin).
- [ ] Le bouton `[+]` ouvre l'expansion inline et instancie le bon template au clic.
- [ ] Cliquer sur un agent dans la sidebar bascule l'affichage central sur son terminal.
- [ ] Plusieurs agents peuvent être actifs simultanément (tous drainés au tick 8ms).
