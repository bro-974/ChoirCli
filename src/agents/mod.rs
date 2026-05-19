use std::sync::{Arc, Mutex, mpsc};
use std::path::Path;
use crate::terminal::{PtyHandle, TerminalEmulator, spawn_pty};
use crate::db::{Project, AgentTemplate};

fn resolve_args(args: &[String], session_name: &str) -> Vec<String> {
    args.iter()
        .map(|a| a.replace("{session_name}", session_name))
        .collect()
}

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
                resolve_args(&template.base_args, &instance.custom_name)
            }
        } else {
            resolve_args(&template.base_args, &instance.custom_name)
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

    pub fn close(&mut self, id: &str) -> Option<String> {
        if let Some(pos) = self.agents.iter().position(|a| a.id == id) {
            let mut agent = self.agents.remove(pos);
            let _ = agent.pty.child.kill();
            if self.focused_id.as_deref() == Some(id) {
                self.focused_id = None;
            }
            Some(agent.instance_id)
        } else {
            None
        }
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
            resume_arg: String::new(),
        };
        pool.spawn(&project, &template, "test-instance", "Test 14h22", 80, 24);
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
            resume_arg: String::new(),
        };
        pool.spawn(&project, &template, "test-instance", "Test 14h22", 80, 24);
        pool.spawn(&project, &template, "test-instance", "Test 14h22", 80, 24);
        let first_id = pool.agents[0].id.clone();
        pool.focus(&first_id);
        assert_eq!(pool.focused_id.as_deref(), Some(first_id.as_str()));
        let second_id = pool.agents[1].id.clone();
        pool.focus(&second_id);
        assert_eq!(pool.focused_id.as_deref(), Some(second_id.as_str()));
    }
}
