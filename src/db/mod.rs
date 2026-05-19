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
    pub resume_arg: String,
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
        // Idempotent — silently ignored if column already exists
        conn.execute_batch(
            "ALTER TABLE agent_templates ADD COLUMN resume_arg TEXT NOT NULL DEFAULT '';"
        ).ok();
        Self::seed_templates(conn)?;
        Self::migrate_templates(conn)?;
        Ok(())
    }

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
            .prepare(
                "SELECT id, name, cli_command, base_args, default_prompt, resume_arg
                 FROM agent_templates ORDER BY name"
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

    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = inner.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if !in_quotes => { in_quotes = true; }
            '"' if in_quotes => { in_quotes = false; }
            '\\' if in_quotes => {
                if let Some(next) = chars.next() {
                    match next {
                        '"' => current.push('"'),
                        '\\' => current.push('\\'),
                        other => { current.push('\\'); current.push(other); }
                    }
                }
            }
            ',' if !in_quotes => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => { if in_quotes { current.push(ch); } }
        }
    }
    if !current.trim().is_empty() || in_quotes {
        result.push(current.trim().to_string());
    }
    result
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
        assert_eq!(t[0].base_args, vec!["-n", "{session_name}"]);
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

    #[test]
    fn json_roundtrip_with_comma() {
        let enc = json_encode(&["--flag=a,b", "normal"]);
        let dec = json_decode(&enc);
        assert_eq!(dec, vec!["--flag=a,b", "normal"]);
    }

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
}
