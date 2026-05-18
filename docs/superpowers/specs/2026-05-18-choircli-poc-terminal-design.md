# Design : POC Intégration Terminal (ChoirCli)

**Date** : 2026-05-18  
**Statut** : Approuvé

---

## 1. Objectif

Prototype fonctionnel en Rust démontrant la capacité de l'application à instancier un pseudo-terminal (PTY), y exécuter un shell interactif, et restituer le rendu visuel (couleurs ANSI TrueColor) dans une interface graphique native `iced`, avec transmission intégrale des entrées clavier.

---

## 2. Choix techniques retenus

| Composant | Choix | Raison |
|---|---|---|
| GUI | `iced 0.13` avec `Canvas` | Rendu custom pixel-perfect de la grille, modèle Elm natif |
| PTY | `portable-pty 0.8` | Multiplateforme Unix/Windows |
| ANSI parser | `vte 0.13` | Parseur bas niveau, contrôle total de la grille |
| Police | JetBrainsMono Nerd Font via `include_bytes!` | Pas de dépendance système |
| Intégration PTY↔iced | Thread OS + `mpsc` + `iced::subscription::channel` | Simple, sans async runtime, parfait pour POC |

---

## 3. Architecture générale

```
┌─────────────────────────────────────────────────────┐
│                  iced Application                   │
│                                                     │
│  ┌──────────────┐    Message::PtyOutput(Vec<u8>)   │
│  │ Subscription │◄──────────────────────────────┐  │
│  └──────┬───────┘                               │  │
│         │ update()                              │  │
│  ┌──────▼───────┐     ┌──────────────┐          │  │
│  │  AppState    │────►│TerminalScreen│          │  │
│  │  (vte parse) │     │  Grid/Cells  │          │  │
│  └──────────────┘     └──────┬───────┘          │  │
│                              │ view()            │  │
│                       ┌──────▼───────┐           │  │
│                       │CanvasWidget  │           │  │
│                       │ (Nerd Font)  │           │  │
│                       └──────┬───────┘           │  │
│                              │ KeyboardEvent      │  │
│                              ▼                    │  │
│                       Message::KeyInput(Vec<u8>) │  │
└───────────────────────────────┼───────────────────┘  │
                                │ PtyHandle::write()
                    ┌───────────▼─────────┐
                    │  Thread PTY Reader   │
                    │  (std::thread, loop) │──┘ mpsc::Sender
                    └───────────┬─────────┘
                                │ portable-pty
                    ┌───────────▼─────────┐
                    │  Processus enfant    │
                    │  (shell système)     │
                    └─────────────────────┘
```

**Flux de données :**
1. Thread PTY lit octets bruts → `mpsc::Sender<Vec<u8>>`
2. `Subscription` iced reçoit → `Message::PtyOutput`
3. `update()` passe les octets à `TerminalScreen::process()` → `vte::Parser` → met à jour la `Grid`
4. `view()` dessine la grille via `Canvas` avec la Nerd Font embarquée
5. Frappe clavier → `Message::KeyInput` → `PtyHandle::write()` → master PTY

---

## 4. Structure des fichiers

```
src/
├── main.rs
├── terminal/
│   ├── mod.rs
│   ├── pty.rs        # PtyHandle, spawn_pty(), thread lecteur
│   └── screen.rs     # TerminalScreen, Cell, impl vte::Perform
└── ui/
    ├── mod.rs
    ├── app.rs        # App iced, Message, update(), subscription()
    └── terminal_widget.rs  # impl canvas::Program, rendu grille
assets/
└── JetBrainsMonoNerdFont-Regular.ttf
```

---

## 5. Détail des modules

### 5.1 `src/terminal/pty.rs`

```rust
pub struct PtyHandle {
    master: Box<dyn MasterPty>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn ChildKiller>,
}

pub fn spawn_pty(cols: u16, rows: u16) -> (PtyHandle, mpsc::Receiver<Vec<u8>>)
```

- **Initialisation** : `portable_pty::native_pty_system()` → `openpty(PtySize { cols, rows })` → spawn du shell système (`$SHELL` sur Unix, `cmd.exe` sur Windows).
- **Thread lecteur** : boucle `master.read(&mut buf)` → `mpsc::Sender`. Arrêt propre si read retourne 0.
- **`write(&[u8])`** : injection directe dans le master PTY.
- **`resize(cols, rows)`** : `master.resize(PtySize { cols, rows })` — `portable-pty` gère `SIGWINCH` automatiquement.

### 5.2 `src/terminal/screen.rs`

```rust
#[derive(Clone)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,       // RGB 24-bit
    pub bg: Color,
    pub bold: bool,
    pub underline: bool,
}

pub struct TerminalScreen {
    grid: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    cols: usize,
    rows: usize,
    parser: vte::Parser,
}
```

**`impl vte::Perform` sur `TerminalScreen` :**

| Callback | Action |
|---|---|
| `print(char)` | Écrit le char à la position curseur, avance |
| `execute(\n)` | Saut de ligne + scroll si fin de grille |
| `execute(\r)` | Retour chariot |
| `csi_dispatch (SGR)` | Couleurs 8/256/TrueColor, gras, souligné, reset |
| `csi_dispatch (CUP/CUU/CUD/CUF/CUB)` | Déplacement curseur |

**Défilement** : `grid.remove(0); grid.push(ligne_vide)` quand le curseur dépasse `rows`. Pas de scrollback buffer pour le POC.

**API publique** :
- `process(&mut self, bytes: &[u8])` — avance le parseur vte octet par octet
- `grid(&self) -> &Vec<Vec<Cell>>` — accès lecture pour le Canvas
- `resize(&mut self, cols: usize, rows: usize)` — recalcule la grille

### 5.3 `src/ui/terminal_widget.rs`

Implémente `iced::widget::canvas::Program` :

- **Rendu** : pour chaque `Cell`, rectangle de fond (`bg`) + `Frame::fill_text()` pour le glyphe.
- **Position** : `x = col * char_width`, `y = row * char_height`.
- **Dimensions caractère** : calculées une fois (`char_width ≈ font_size * 0.6`, `char_height = font_size`).
- **Police** : `static FONT_BYTES: &[u8] = include_bytes!("../../assets/JetBrainsMonoNerdFont-Regular.ttf")`.

**Capture clavier** (via `canvas::Event::Keyboard`) → conversions :

| Touche | Séquence PTY |
|---|---|
| `Enter` | `\r` |
| `Backspace` | `\x7f` |
| `Ctrl+C` | `\x03` |
| `Ctrl+D` | `\x04` |
| `Tab` | `\t` |
| `Flèches` | `\x1b[A/B/C/D` |
| Caractère imprimable | UTF-8 brut |

### 5.4 `src/ui/app.rs`

```rust
enum Message {
    PtyOutput(Vec<u8>),
    KeyInput(Vec<u8>),
    Resize(u16, u16),
    FontLoaded,
}
```

- **`subscription()`** : `iced::subscription::channel` wrappant le `mpsc::Receiver`.
- **Resize** : écoute `iced::Event::Window::Resized` + `MouseButtonReleased` → calcule dimensions en caractères → `PtyHandle::resize()`.

---

## 6. Dépendances Cargo

```toml
[dependencies]
iced       = { version = "0.13", features = ["canvas", "advanced"] }
portable-pty = "0.8"
vte        = "0.13"
```

---

## 7. Critères d'acceptation

- [ ] Aucun freeze GUI lors de flux textuels rapides
- [ ] Toutes les frappes transmises sans interception
- [ ] Police Nerd Font embarquée dans le binaire (pas de dépendance système)
- [ ] Pas de `panic!` non géré dans les threads PTY
- [ ] Couleurs TrueColor correctement rendues
- [ ] Resize stable après relâchement de la souris
