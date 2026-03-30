use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    Setup,
    Normal,
    Recovery,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Normal => "normal",
            Self::Recovery => "recovery",
        }
    }

    pub fn target_name(self) -> &'static str {
        match self {
            Self::Setup => "clawpi-setup.target",
            Self::Normal => "clawpi.target",
            Self::Recovery => "clawpi-recovery.target",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn detect() -> Self {
        let root = env::var_os("CLAWPI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self { root }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn etc_dir(&self) -> PathBuf {
        self.root.join("etc").join("clawpi")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("var").join("lib").join("clawpi")
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run").join("clawpi")
    }

    pub fn setup_complete_path(&self) -> PathBuf {
        self.state_dir().join("setup-complete")
    }

    pub fn recovery_requested_path(&self) -> PathBuf {
        self.state_dir().join("recovery-requested")
    }

    pub fn setup_state_path(&self) -> PathBuf {
        self.state_dir().join("setup-state")
    }

    pub fn active_mode_path(&self) -> PathBuf {
        self.run_dir().join("active-mode")
    }

    pub fn last_mode_path(&self) -> PathBuf {
        self.state_dir().join("last-mode")
    }

    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.etc_dir())?;
        fs::create_dir_all(self.state_dir())?;
        fs::create_dir_all(self.run_dir())?;
        Ok(())
    }
}

pub fn detect_mode(layout: &Layout) -> Mode {
    if layout.recovery_requested_path().exists() {
        Mode::Recovery
    } else if layout.setup_complete_path().exists() {
        Mode::Normal
    } else {
        Mode::Setup
    }
}

pub fn mark_setup_complete(layout: &Layout, complete: bool) -> io::Result<()> {
    layout.ensure_dirs()?;

    if complete {
        fs::write(layout.setup_complete_path(), b"phase=2\nstatus=complete\n")?;
    } else {
        remove_if_exists(&layout.setup_complete_path())?;
    }

    Ok(())
}

pub fn set_recovery_requested(layout: &Layout, requested: bool) -> io::Result<()> {
    layout.ensure_dirs()?;

    if requested {
        fs::write(
            layout.recovery_requested_path(),
            b"phase=2\nstatus=requested\n",
        )?;
    } else {
        remove_if_exists(&layout.recovery_requested_path())?;
    }

    Ok(())
}

pub fn record_mode(layout: &Layout, mode: Mode) -> io::Result<()> {
    layout.ensure_dirs()?;
    fs::write(layout.active_mode_path(), format!("{}\n", mode.as_str()))?;
    fs::write(layout.last_mode_path(), format!("{}\n", mode.as_str()))?;
    Ok(())
}

pub fn write_setup_state(layout: &Layout) -> io::Result<Mode> {
    layout.ensure_dirs()?;

    let mode = detect_mode(layout);
    let status = match mode {
        Mode::Setup => "pending",
        Mode::Normal => "complete",
        Mode::Recovery => "recovery",
    };
    let note = match mode {
        Mode::Setup => "proving-ground setup flow not implemented yet",
        Mode::Normal => "setup-complete marker present",
        Mode::Recovery => "recovery mode requested",
    };

    let content = format!(
        "phase=2\nmode={}\nstatus={}\nnote={}\n",
        mode.as_str(),
        status,
        note
    );
    fs::write(layout.setup_state_path(), content)?;

    Ok(mode)
}

pub fn read_optional_file(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content.trim().to_string())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn setup_is_default_without_markers() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        assert_eq!(detect_mode(&layout), Mode::Setup);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_overrides_setup_complete() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        mark_setup_complete(&layout, true).unwrap();
        set_recovery_requested(&layout, true).unwrap();

        assert_eq!(detect_mode(&layout), Mode::Recovery);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn record_mode_writes_runtime_files() {
        let root = unique_test_root();
        let layout = Layout::from_root(&root);

        record_mode(&layout, Mode::Normal).unwrap();

        assert_eq!(
            read_optional_file(&layout.active_mode_path()).unwrap(),
            Some(String::from("normal"))
        );
        assert_eq!(
            read_optional_file(&layout.last_mode_path()).unwrap(),
            Some(String::from("normal"))
        );

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("clawpi-core-test-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
