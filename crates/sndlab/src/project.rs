//! Project model — a directory of `.rhai` script files plus a
//! `project.ron` manifest.
//!
//! A *project* is the thing the user opens, edits, and saves as a
//! unit. Inside, individual `.rhai` files are *scripts* — they
//! contribute patches to a shared namespace. Evaluating a project
//! concatenates all scripts and runs them through the engine, so
//! patches in `weapons.rhai` and `ambience.rhai` can be triggered
//! from the same toolbar with no inter-file fuss.
//!
//! The save/load surface deliberately stays small. A manifest holds
//! the project name and the list of script files (relative paths).
//! Future per-patch overrides for the mix model (task 10) extend the
//! manifest in place; for now the schema is just `name` and
//! `scripts`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Persisted form of a project. Lives at `<project_dir>/project.ron`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    /// Script paths relative to the project directory, evaluated in
    /// list order.
    pub scripts: Vec<String>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            name: "untitled".into(),
            scripts: vec!["patches.rhai".into()],
        }
    }
}

/// A script that's been loaded into memory and is editable.
#[derive(Debug, Clone)]
pub struct Script {
    pub relative_path: String,
    pub buffer: String,
    /// `true` if the in-memory buffer differs from what's on disk
    /// (or, for unsaved projects, from its construction-time value).
    pub dirty: bool,
}

/// An in-memory project, ready to edit. Carries every loaded script,
/// the index of the currently-active script (the one shown in the
/// editor pane), and an optional project root directory. `root` is
/// `None` for unsaved projects (created in memory) and `Some` once
/// the project has been opened from or saved to disk.
#[derive(Debug, Clone)]
pub struct Project {
    pub manifest: Manifest,
    pub root: Option<PathBuf>,
    pub scripts: Vec<Script>,
    pub active: usize,
}

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Ron(String),
    EmptyManifest,
    MissingScript(String),
    InvalidFilename(String),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Ron(s) => write!(f, "ron: {s}"),
            Self::EmptyManifest => write!(f, "no scripts in project manifest"),
            Self::MissingScript(s) => write!(f, "script '{s}' not found relative to project"),
            Self::InvalidFilename(s) => write!(f, "invalid filename: {s}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ron::error::SpannedError> for ProjectError {
    fn from(value: ron::error::SpannedError) -> Self {
        Self::Ron(value.to_string())
    }
}

impl From<ron::Error> for ProjectError {
    fn from(value: ron::Error) -> Self {
        Self::Ron(value.to_string())
    }
}

const MANIFEST_FILENAME: &str = "project.ron";

impl Project {
    /// Build an unsaved, in-memory project from a single seed script.
    /// Useful as the default startup project so the user has
    /// something they can press F5 on immediately.
    pub fn unsaved(name: impl Into<String>, seed: String) -> Self {
        let name = name.into();
        let manifest = Manifest {
            name,
            scripts: vec!["patches.rhai".into()],
        };
        Self {
            manifest,
            root: None,
            scripts: vec![Script {
                relative_path: "patches.rhai".into(),
                buffer: seed,
                dirty: false,
            }],
            active: 0,
        }
    }

    /// Open a project directory. Reads `project.ron` and loads each
    /// listed script into memory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let root = root.as_ref().to_path_buf();
        let manifest_path = root.join(MANIFEST_FILENAME);
        let manifest_str = fs::read_to_string(&manifest_path)?;
        let manifest: Manifest = ron::from_str(&manifest_str)?;
        Self::load_scripts(root, manifest)
    }

    /// Build a project from a directory that doesn't yet have a
    /// manifest. Treats every `.rhai` file directly under `root` as
    /// a script. Useful for "open this folder as a project" when no
    /// manifest exists yet — the next save writes one.
    pub fn open_directory(root: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let root = root.as_ref().to_path_buf();
        let mut scripts: Vec<String> = fs::read_dir(&root)?
            .filter_map(|entry| entry.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rhai"))
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.to_string())
            })
            .collect();
        scripts.sort();
        if scripts.is_empty() {
            return Err(ProjectError::EmptyManifest);
        }
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();
        let manifest = Manifest { name, scripts };
        Self::load_scripts(root, manifest)
    }

    fn load_scripts(root: PathBuf, manifest: Manifest) -> Result<Self, ProjectError> {
        if manifest.scripts.is_empty() {
            return Err(ProjectError::EmptyManifest);
        }
        let mut scripts = Vec::with_capacity(manifest.scripts.len());
        for rel in &manifest.scripts {
            let abs = root.join(rel);
            if !abs.is_file() {
                return Err(ProjectError::MissingScript(rel.clone()));
            }
            let buffer = fs::read_to_string(&abs)?;
            scripts.push(Script {
                relative_path: rel.clone(),
                buffer,
                dirty: false,
            });
        }
        Ok(Self {
            manifest,
            root: Some(root),
            scripts,
            active: 0,
        })
    }

    /// Concatenate every script's buffer with a separator comment.
    /// This is what gets handed to the engine to evaluate.
    pub fn concatenated_source(&self) -> String {
        let mut out = String::new();
        for s in &self.scripts {
            out.push_str(&format!("// ─── {} ───\n", s.relative_path));
            out.push_str(&s.buffer);
            if !s.buffer.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Write every dirty script back to disk and (re)write the
    /// manifest. Errors if the project has no root yet (use
    /// `save_to` to pick a destination first).
    pub fn save(&mut self) -> Result<(), ProjectError> {
        let Some(root) = self.root.clone() else {
            return Err(ProjectError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsaved project — pick a directory with Save As first",
            )));
        };
        self.save_to(&root)
    }

    /// Save the project to a specific directory. Sets `root` to the
    /// destination on success so subsequent `save` calls go to the
    /// same place.
    pub fn save_to(&mut self, root: &Path) -> Result<(), ProjectError> {
        fs::create_dir_all(root)?;
        let manifest_path = root.join(MANIFEST_FILENAME);
        let pretty = ron::ser::PrettyConfig::new()
            .struct_names(false)
            .new_line("\n".into());
        let manifest_str = ron::ser::to_string_pretty(&self.manifest, pretty)?;
        fs::write(&manifest_path, manifest_str + "\n")?;
        for script in &mut self.scripts {
            let abs = root.join(&script.relative_path);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, &script.buffer)?;
            script.dirty = false;
        }
        self.root = Some(root.to_path_buf());
        Ok(())
    }

    /// `true` if any script's in-memory buffer differs from disk.
    pub fn is_dirty(&self) -> bool {
        self.scripts.iter().any(|s| s.dirty)
    }

    /// Convenience: the currently-active script's buffer.
    pub fn active_buffer(&self) -> &str {
        &self.scripts[self.active].buffer
    }

    /// Mutable access to the active script's buffer. Does NOT mark
    /// the script dirty by itself — call `mark_active_dirty` or use
    /// `set_active_buffer` when you actually modify it. The UI
    /// snapshots before/after and only marks dirty on real change.
    pub fn active_buffer_mut(&mut self) -> &mut String {
        &mut self.scripts[self.active].buffer
    }

    pub fn mark_active_dirty(&mut self) {
        self.scripts[self.active].dirty = true;
    }

    /// Replace the active script's buffer wholesale (used by the
    /// MCP `set_buffer` tool and by file-tab switches that don't
    /// otherwise edit). Only marks dirty if the content actually
    /// changes.
    pub fn set_active_buffer(&mut self, content: String) {
        let s = &mut self.scripts[self.active];
        if s.buffer != content {
            s.buffer = content;
            s.dirty = true;
        }
    }

    /// Add a new (empty) script to the project. Validates the
    /// filename and refuses collisions. Updates the manifest. Returns
    /// the new script's index — the caller usually wants to make it
    /// the active tab.
    pub fn add_script(&mut self, filename: String) -> Result<usize, ProjectError> {
        let filename = filename.trim().to_string();
        if filename.is_empty() {
            return Err(ProjectError::InvalidFilename(
                "filename is empty".into(),
            ));
        }
        if filename.contains('/') || filename.contains('\\') {
            return Err(ProjectError::InvalidFilename(
                "no path separators allowed".into(),
            ));
        }
        if self
            .scripts
            .iter()
            .any(|s| s.relative_path == filename)
        {
            return Err(ProjectError::InvalidFilename(format!(
                "'{filename}' already exists in the project"
            )));
        }
        self.manifest.scripts.push(filename.clone());
        self.scripts.push(Script {
            relative_path: filename,
            buffer: "// new script — define patches with patch(name, role, signal)\n"
                .to_string(),
            dirty: true,
        });
        Ok(self.scripts.len() - 1)
    }

    /// Rename a script's file. If the project has a root on disk and
    /// the file already exists there, the rename is performed on disk
    /// too. Updates the manifest's script list and the in-memory
    /// script's relative path.
    pub fn rename_script(
        &mut self,
        index: usize,
        new_filename: String,
    ) -> Result<(), ProjectError> {
        let new_filename = new_filename.trim().to_string();
        if new_filename.is_empty() {
            return Err(ProjectError::InvalidFilename(
                "filename is empty".into(),
            ));
        }
        if new_filename.contains('/') || new_filename.contains('\\') {
            return Err(ProjectError::InvalidFilename(
                "no path separators allowed".into(),
            ));
        }
        if index >= self.scripts.len() {
            return Err(ProjectError::InvalidFilename(format!(
                "no script at index {index}"
            )));
        }
        if self
            .scripts
            .iter()
            .enumerate()
            .any(|(i, s)| i != index && s.relative_path == new_filename)
        {
            return Err(ProjectError::InvalidFilename(format!(
                "'{new_filename}' already exists in the project"
            )));
        }
        let old_filename = self.scripts[index].relative_path.clone();
        if let Some(root) = &self.root {
            let old_path = root.join(&old_filename);
            let new_path = root.join(&new_filename);
            if old_path.is_file() {
                fs::rename(&old_path, &new_path)?;
            }
        }
        self.scripts[index].relative_path = new_filename.clone();
        self.manifest.scripts[index] = new_filename;
        // Don't mark dirty — the rename is already persisted on disk
        // and the manifest will be rewritten on the next save anyway.
        Ok(())
    }

    /// Remove a script from the project. Deletes the file from disk
    /// (if the project has a root and the file exists). Updates the
    /// manifest and adjusts the active index if necessary.
    /// Refuses to delete the last remaining script — a project
    /// without scripts has no editor to render into.
    pub fn delete_script(&mut self, index: usize) -> Result<(), ProjectError> {
        if self.scripts.len() <= 1 {
            return Err(ProjectError::InvalidFilename(
                "can't delete the last script in a project".into(),
            ));
        }
        if index >= self.scripts.len() {
            return Err(ProjectError::InvalidFilename(format!(
                "no script at index {index}"
            )));
        }
        let filename = self.scripts[index].relative_path.clone();
        if let Some(root) = &self.root {
            let path = root.join(&filename);
            if path.is_file() {
                fs::remove_file(&path)?;
            }
        }
        self.scripts.remove(index);
        self.manifest.scripts.remove(index);
        // Keep active in bounds; prefer staying at the same visual
        // position when possible.
        if self.active >= self.scripts.len() {
            self.active = self.scripts.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        Ok(())
    }
}
