# Design : Run 4 — Session Resume

**Date** : 2026-05-19
**Statut** : Approuvé

---

## 1. Objectif

Implémenter la restauration automatique des agents au démarrage : chaque instance présente en base est respawnée, en utilisant `--resume <session_name>` pour les templates qui le supportent (Claude), ou en fresh start silencieux pour les autres. Ajouter la fermeture d'agent (suppression DB + SIGTERM).

---

## 2. Approche retenue

**Option A — Deux chemins explicites (`spawn` vs `restore`)** : `AgentPool` expose deux méthodes distinctes. `spawn()` est utilisé pour les nouveaux agents créés manuellement (fresh start). `restore()` est utilisé au démarrage depuis la DB (resume si possible, sinon fresh start silencieux). La logique de choix est dans `AgentPool`, pas dans `App`.

---

## 3. Couche DB (`src/db/mod.rs`)

### Migration de schéma

Au démarrage, `init_schema` exécute en plus :

```sql
ALTER TABLE agent_templates ADD COLUMN resume_arg TEXT NOT NULL DEFAULT '';
```

SQLite supporte `ALTER TABLE ADD COLUMN` sur une DB existante de façon non-destructive ; les lignes existantes héritent la valeur `''`.

### Mise à jour du seed

Le template `claude-main` reçoit `resume_arg = "--resume"`. Comme le seed est protégé par `IF count == 0`, une migration séparée corrige les lignes existantes :

```sql
UPDATE agent_templates SET resume_arg = '--resume' WHERE id = 'claude-main' AND resume_arg = '';
```

Les `base_args` du seed sont mis à jour : `["-n", "{session_name}"]` — le placeholder `{session_name}` est résolu au moment du spawn vers le `custom_name` généré. Une migration corrige également les DBs existantes dont la valeur était codée en dur `"Main"` :

```sql
UPDATE agent_templates SET base_args = '["-n","{session_name}"]'
WHERE id = 'claude-main' AND base_args = '["-n","Main"]';
```

### `custom_name` unique

Généré dans `insert_instance` avec le pattern `"<template.name> <HH>h<MM>"` (ex: `"Claude Main 14h22"`). Si le nom existe déjà en base, on boucle avec suffixe ` 2`, ` 3`, etc. (le suffixe ` 1` n'est jamais affiché). Stocké dans `custom_name` **et** dans `last_session_id`.

### Types Rust mis à jour

```rust
pub struct AgentTemplate {
    pub id: String,
    pub name: String,
    pub cli_command: String,
    pub base_args: Vec<String>,
    pub default_prompt: String,
    pub resume_arg: String,   // nouveau — "" si pas de support resume
}
```

### Nouvelle API DB

```rust
// Retourne toutes les instances avec leur projet et template associés
pub fn list_instances_with_context(&self) -> Vec<(AgentInstance, Project, AgentTemplate)>

// Supprime une instance (fermeture définitive)
pub fn delete_instance(&self, id: &str)
```

---

## 4. Orchestration des agents (`src/agents/mod.rs`)

### Deux méthodes de spawn

```rust
// Nouveau spawn manuel (fresh start)
pub fn spawn(
    &mut self,
    project: &Project,
    template: &AgentTemplate,
    instance_id: &str,
    session_name: &str,
    cols: u16,
    rows: u16,
)

// Restore au démarrage depuis la DB
pub fn restore(
    &mut self,
    project: &Project,
    template: &AgentTemplate,
    instance: &AgentInstance,
    cols: u16,
    rows: u16,
)
```

**`spawn()`** résout `{session_name}` dans `base_args` puis lance `cli_command + resolved_args`.

**`restore()`** :
- `template.resume_arg` non vide **ET** `instance.last_session_id` non nul → commande : `cli_command resume_arg last_session_id`
- sinon → `cli_command + base_args` (fresh start silencieux, sans erreur)

### Résolution des placeholders

Fonction utilitaire dans `agents/mod.rs` :

```rust
fn resolve_args(args: &[String], session_name: &str) -> Vec<String>
// Remplace "{session_name}" par session_name dans chaque élément
```

### Fermeture d'un agent

```rust
pub fn close(&mut self, id: &str) -> Option<String>
// Envoie SIGTERM au processus enfant (child.kill()),
// retire l'agent du Vec, retourne l'instance_id pour suppression DB
```

### `ActiveAgent` — nouveau champ

```rust
pub struct ActiveAgent {
    pub id: String,
    pub instance_id: String,   // nouveau — FK vers project_instances.id
    pub project_id: String,
    pub template_name: String,
    pub spawned_at: String,
    pub emulator: TerminalEmulator,
    pub pty: PtyHandle,
    pub pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}
```

---

## 5. Application (`src/ui/app.rs`)

### Restore dans `App::new()`

```rust
let instances = db.list_instances_with_context();
for (instance, project, template) in &instances {
    pool.restore(&project, &template, &instance, 80, 24);
}
```

Les dimensions initiales `(80, 24)` sont corrigées dès le premier `WindowResized`.

### Nouveau message

```rust
CloseAgent(String),  // agent_id
```

### Handler `CloseAgent`

1. `pool.close(id)` → récupère `instance_id`
2. `db.delete_instance(&instance_id)`
3. Si l'agent fermé était le focused → `pool.focused_id = None`

### Handler `SpawnAgent` mis à jour

Passe `instance.id` et `instance.custom_name` (= `last_session_id`) à `pool.spawn()`.

---

## 6. Interface utilisateur (`src/ui/sidebar.rs`)

Chaque ligne agent reçoit un bouton `[×]` à droite :

```
🤖 Claude Main (14h22)   [×]
```

Le `[×]` émet `Message::CloseAgent(agent.id.clone())`. Pas de confirmation — fermeture directe.

---

## 7. Structure des fichiers modifiés

```
src/
├── db/mod.rs               ALTER TABLE, migration seed, list_instances_with_context,
│                           delete_instance, génération custom_name unique
├── agents/mod.rs           spawn() + restore() + close(), resolve_args(),
│                           nouveaux champs ActiveAgent
└── ui/
    ├── app.rs              Restore dans new(), CloseAgent handler, SpawnAgent mis à jour
    └── sidebar.rs          Bouton [×] sur chaque agent actif
```

Aucune nouvelle dépendance — `chrono` (déjà présent) pour le format `HHhMM`.

---

## 8. Critères d'acceptation

- [ ] Cohérence après redémarrage : tous les agents actifs avant fermeture réapparaissent dans la sidebar.
- [ ] `--resume` fonctionnel : Claude reprend la conversation de la session précédente.
- [ ] Fresh start silencieux : un template sans `resume_arg` s'ouvre à blanc sans bloquer l'app.
- [ ] `custom_name` unique : deux agents spawnés à la même minute ont des noms distincts (suffixe ` 2`).
- [ ] Fermeture propre : clic `[×]` → SIGTERM, suppression DB, pas de restore au prochain démarrage.
- [ ] Focus reset : si l'agent fermé était focused, le workspace affiche "Sélectionne un agent".
