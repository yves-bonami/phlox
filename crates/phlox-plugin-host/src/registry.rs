//! Versioned local plugin installations and live discovery.

use crate::{LoadError, LoadedPlugin};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub library: String,
    pub abi_version: u32,
}

#[derive(Debug)]
pub enum RegistryError {
    Io(io::Error),
    Invalid(String),
    Load(LoadError),
    Watch(notify::Error),
}

impl From<io::Error> for RegistryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<LoadError> for RegistryError {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}
impl From<notify::Error> for RegistryError {
    fn from(error: notify::Error) -> Self {
        Self::Watch(error)
    }
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn manifest(directory: &Path) -> Result<Manifest, RegistryError> {
    let contents = fs::read_to_string(directory.join("plugin.toml"))?;
    let parsed: Manifest =
        toml::from_str(&contents).map_err(|e| RegistryError::Invalid(e.to_string()))?;
    if !valid_component(&parsed.id)
        || !valid_component(&parsed.version)
        || !valid_component(&parsed.library)
        || !parsed.library.ends_with(".so")
        || parsed.abi_version != phlox_plugin_api::ABI_VERSION
    {
        return Err(RegistryError::Invalid(
            "invalid manifest identity, library or ABI".into(),
        ));
    }
    let file = directory.join(&parsed.library);
    if !file.is_file() || fs::symlink_metadata(file)?.file_type().is_symlink() {
        return Err(RegistryError::Invalid(
            "plugin binary is missing or a symlink".into(),
        ));
    }
    Ok(parsed)
}

/// A loaded plugin remains mapped for the entire registry lifetime.
/// New IDs load immediately. Updates of active IDs are deferred until restart.
pub struct PluginRegistry {
    root: PathBuf,
    active: BTreeMap<String, LoadedPlugin>,
}

impl PluginRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            active: BTreeMap::new(),
        }
    }

    pub fn active(&self) -> impl Iterator<Item = &crate::PluginInfo> {
        self.active.values().map(|plugin| &plugin.info)
    }

    /// Scan installed immutable versions. Failures are reported per version;
    /// a broken package does not prevent loading other plugins.
    pub fn refresh(&mut self) -> Vec<(PathBuf, RegistryError)> {
        let mut failures = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return failures,
            Err(error) => return vec![(self.root.clone(), RegistryError::Io(error))],
        };
        for plugin_dir in entries.flatten() {
            let root = plugin_dir.path();
            if !root.is_dir() || plugin_dir.file_type().is_ok_and(|t| t.is_symlink()) {
                continue;
            }
            let Ok(versions) = fs::read_dir(&root) else {
                continue;
            };
            let mut versions: Vec<_> = versions.flatten().collect();
            versions.sort_by_key(|e| e.file_name());
            versions.reverse();
            for entry in versions {
                let directory = entry.path();
                if !directory.is_dir() || entry.file_type().is_ok_and(|t| t.is_symlink()) {
                    continue;
                }
                if directory
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                {
                    continue;
                }
                let result = (|| {
                    let meta = manifest(&directory)?;
                    if root.file_name().is_none_or(|name| name != meta.id.as_str())
                        || directory
                            .file_name()
                            .is_none_or(|name| name != meta.version.as_str())
                    {
                        return Err(RegistryError::Invalid(
                            "manifest does not match install path".into(),
                        ));
                    }
                    if self.active.contains_key(&meta.id) {
                        return Ok(());
                    }
                    // SAFETY: Only trusted local plugin packages can be installed. See ABI docs.
                    let plugin = unsafe { crate::load(directory.join(&meta.library)) }?;
                    if plugin.info.id != meta.id || plugin.info.version != meta.version {
                        return Err(RegistryError::Invalid(
                            "plugin metadata differs from manifest".into(),
                        ));
                    }
                    self.active.insert(meta.id, plugin);
                    Ok(())
                })();
                if let Err(error) = result {
                    failures.push((directory, error));
                }
            }
        }
        failures
    }

    /// Install a local, trusted package, atomically publishing the completed version.
    /// Existing versions are never overwritten.
    pub fn install(&self, source: &Path) -> Result<PathBuf, RegistryError> {
        let meta = manifest(source)?;
        fs::create_dir_all(&self.root)?;
        let plugin_root = self.root.join(&meta.id);
        fs::create_dir_all(&plugin_root)?;
        let destination = plugin_root.join(&meta.version);
        if destination.exists() {
            return Err(RegistryError::Invalid("version already installed".into()));
        }
        let stage = plugin_root.join(format!(".{}.staging-{}", meta.version, std::process::id()));
        fs::create_dir(&stage)?;
        let result = (|| {
            fs::copy(source.join("plugin.toml"), stage.join("plugin.toml"))?;
            fs::copy(source.join(&meta.library), stage.join(&meta.library))?;
            fs::rename(&stage, &destination)?;
            Ok::<_, io::Error>(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&stage);
            return Err(error.into());
        }
        Ok(destination)
    }

    /// Block on filesystem events and refresh; callback reports newly active plugins.
    /// Keep the watcher alive while the application runs.
    pub fn watch(
        mut self,
        mut on_change: impl FnMut(&Self) + Send + 'static,
    ) -> Result<(), RegistryError> {
        fs::create_dir_all(&self.root)?;
        let (sender, receiver) = mpsc::channel();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |result| {
            let _ = sender.send(result);
        })?;
        watcher.watch(&self.root, RecursiveMode::Recursive)?;
        self.refresh();
        on_change(&self);
        while let Ok(event) = receiver.recv() {
            if event.is_ok() {
                let old = self.active.len();
                self.refresh();
                if self.active.len() != old {
                    on_change(&self);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "phlox-registry-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn install_rejects_invalid_and_duplicate_packages() {
        let root = temp_root();
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("libexample.so"), b"not a real library").unwrap();
        fs::write(source.join("plugin.toml"), "id = \"../escape\"\nversion = \"0.1.0\"\nlibrary = \"libexample.so\"\nabi_version = 1\n").unwrap();
        let registry = PluginRegistry::new(root.join("installed"));
        assert!(matches!(
            registry.install(&source),
            Err(RegistryError::Invalid(_))
        ));
        fs::write(
            source.join("plugin.toml"),
            "id = \"example\"\nversion = \"0.1.0\"\nlibrary = \"libexample.so\"\nabi_version = 1\n",
        )
        .unwrap();
        let installed = registry.install(&source).unwrap();
        assert!(installed.join("plugin.toml").exists());
        assert!(matches!(
            registry.install(&source),
            Err(RegistryError::Invalid(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refresh_skips_incomplete_staging_directories() {
        let root = temp_root();
        let stage = root.join("plugin").join(".0.1.0.staging-1");
        fs::create_dir_all(&stage).unwrap();
        let mut registry = PluginRegistry::new(&root);
        assert!(registry.refresh().is_empty());
        assert_eq!(registry.active().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
