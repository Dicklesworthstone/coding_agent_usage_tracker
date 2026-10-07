//! Detection of installed `JetBrains` IDEs and their AI Assistant quota
//! files.
//!
//! Ports `JetBrainsIDEDetector` from `CodexBar`. IDE configuration lives in
//! per-version directories (`IntelliJIdea2025.3`, `PyCharm2024.2`, ...)
//! under the platform config directory:
//!
//! - macOS: `~/Library/Application Support/JetBrains` (and `/Google` for
//!   Android Studio)
//! - Linux: `$XDG_CONFIG_HOME/JetBrains` (default `~/.config/JetBrains`),
//!   `~/.local/share/JetBrains`, and `~/.config/Google`
//! - Windows: `%APPDATA%\JetBrains` and `%APPDATA%\Google`

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The quota file inside an IDE's `options` directory.
pub const QUOTA_FILE_NAME: &str = "AIAssistantQuotaManager2.xml";

/// Config directory prefixes and display names, matched case-insensitively
/// in this order.
const IDE_PATTERNS: &[(&str, &str)] = &[
    ("IntelliJIdea", "IntelliJ IDEA"),
    ("PyCharm", "PyCharm"),
    ("WebStorm", "WebStorm"),
    ("GoLand", "GoLand"),
    ("CLion", "CLion"),
    ("DataGrip", "DataGrip"),
    ("RubyMine", "RubyMine"),
    ("Rider", "Rider"),
    ("PhpStorm", "PhpStorm"),
    ("AppCode", "AppCode"),
    ("Fleet", "Fleet"),
    ("AndroidStudio", "Android Studio"),
    ("RustRover", "RustRover"),
    ("Aqua", "Aqua"),
    ("DataSpell", "DataSpell"),
];

/// One IDE configuration directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeInfo {
    /// Display name, e.g. `IntelliJ IDEA`.
    pub name: String,
    /// Version suffix of the directory, e.g. `2025.3`, or `Unknown`.
    pub version: String,
    /// The IDE's config directory.
    pub base_path: PathBuf,
    /// `<base_path>/options/AIAssistantQuotaManager2.xml`.
    pub quota_file_path: PathBuf,
}

impl IdeInfo {
    /// `IntelliJ IDEA 2025.3`.
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// The quota file for an IDE config directory.
#[must_use]
pub fn quota_file_path(ide_base_path: &Path) -> PathBuf {
    ide_base_path.join("options").join(QUOTA_FILE_NAME)
}

/// Recognize an IDE config directory name such as `IntelliJIdea2025.3`.
#[must_use]
pub fn parse_ide_directory(dirname: &str, base_path: &Path) -> Option<IdeInfo> {
    IDE_PATTERNS.iter().find_map(|(prefix, display_name)| {
        let head = dirname.get(..prefix.len())?;
        if !head.eq_ignore_ascii_case(prefix) {
            return None;
        }
        let version = &dirname[prefix.len()..];
        let ide_path = base_path.join(dirname);
        Some(IdeInfo {
            name: (*display_name).to_string(),
            version: if version.is_empty() {
                "Unknown".to_string()
            } else {
                version.to_string()
            },
            quota_file_path: quota_file_path(&ide_path),
            base_path: ide_path,
        })
    })
}

/// Directories that hold IDE config directories, given the platform config
/// and data directories. The data directory adds `JetBrains` only where it
/// differs from the config directory (Linux: `~/.local/share`).
#[must_use]
pub fn config_base_paths(config_dir: Option<&Path>, data_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let candidates = [
        config_dir.map(|d| d.join("JetBrains")),
        data_dir.map(|d| d.join("JetBrains")),
        config_dir.map(|d| d.join("Google")),
    ];
    for path in candidates.into_iter().flatten() {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// [`config_base_paths`] for this machine.
#[must_use]
pub fn default_config_base_paths() -> Vec<PathBuf> {
    directories::BaseDirs::new().map_or_else(Vec::new, |dirs| {
        config_base_paths(Some(dirs.config_dir()), Some(dirs.data_dir()))
    })
}

/// Numeric dotted-version comparison; non-numeric parts are skipped and
/// missing parts count as 0.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<u64> { v.split('.').filter_map(|p| p.parse().ok()).collect() };
    let (pa, pb) = (parts(a), parts(b));
    let len = pa.len().max(pb.len());
    (0..len)
        .map(|i| {
            pa.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&pb.get(i).copied().unwrap_or(0))
        })
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// IDEs with a quota file under `base_paths`, sorted by name, newest
/// version first.
#[must_use]
pub fn detect_installed_ides(base_paths: &[PathBuf]) -> Vec<IdeInfo> {
    let mut ides: Vec<IdeInfo> = base_paths
        .iter()
        .filter_map(|base| std::fs::read_dir(base).ok().map(|entries| (base, entries)))
        .flat_map(|(base, entries)| {
            entries
                .filter_map(std::result::Result::ok)
                .filter_map(move |entry| {
                    let name = entry.file_name();
                    parse_ide_directory(name.to_str()?, base)
                })
        })
        .filter(|ide| ide.quota_file_path.exists())
        .collect();
    ides.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| compare_versions(&b.version, &a.version))
    });
    ides
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The IDE whose quota file changed most recently — the one in use. Falls
/// back to the first detected IDE when no modification time is readable.
#[must_use]
pub fn detect_latest_ide(base_paths: &[PathBuf]) -> Option<IdeInfo> {
    let ides = detect_installed_ides(base_paths);
    let mut latest: Option<(&IdeInfo, SystemTime)> = None;
    for ide in &ides {
        let Some(time) = modified(&ide.quota_file_path) else {
            continue;
        };
        if latest.is_none_or(|(_, best)| time > best) {
            latest = Some((ide, time));
        }
    }
    latest
        .map(|(ide, _)| ide.clone())
        .or_else(|| ides.first().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn add_ide(base: &Path, dirname: &str, with_quota: bool) -> PathBuf {
        let options = base.join(dirname).join("options");
        std::fs::create_dir_all(&options).unwrap();
        let quota = options.join(QUOTA_FILE_NAME);
        if with_quota {
            std::fs::write(&quota, "<application/>").unwrap();
        }
        quota
    }

    fn set_modified(path: &Path, secs_after_epoch: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs_after_epoch))
            .unwrap();
    }

    #[test]
    fn parses_ide_directories() {
        let base = Path::new("/test");
        for (dirname, name, version) in [
            ("IntelliJIdea2024.3", "IntelliJ IDEA", "2024.3"),
            ("PyCharm2024.2", "PyCharm", "2024.2"),
            ("WebStorm2024.1", "WebStorm", "2024.1"),
            ("GoLand2024.3", "GoLand", "2024.3"),
            ("CLion2024.2", "CLion", "2024.2"),
            ("RustRover2024.3", "RustRover", "2024.3"),
            ("AndroidStudio2024.2", "Android Studio", "2024.2"),
            ("Webstorm2024.1", "WebStorm", "2024.1"),
            ("pycharm", "PyCharm", "Unknown"),
        ] {
            let info = parse_ide_directory(dirname, base).unwrap();
            assert_eq!(info.name, name);
            assert_eq!(info.version, version);
            assert_eq!(info.base_path, base.join(dirname));
            assert_eq!(
                info.quota_file_path,
                base.join(dirname).join("options").join(QUOTA_FILE_NAME)
            );
            assert_eq!(info.display_name(), format!("{name} {version}"));
        }
        assert_eq!(parse_ide_directory("consentOptions", base), None);
        assert_eq!(parse_ide_directory("Py", base), None);
        assert_eq!(parse_ide_directory("", base), None);
        assert_eq!(parse_ide_directory("ü", base), None);
    }

    #[test]
    fn compares_dotted_versions_numerically() {
        assert_eq!(compare_versions("2024.10", "2024.9"), Ordering::Greater);
        assert_eq!(compare_versions("2024.3", "2024.3.0"), Ordering::Equal);
        assert_eq!(compare_versions("2025.1", "2024.3"), Ordering::Greater);
        assert_eq!(compare_versions("Unknown", "2024.1"), Ordering::Less);
    }

    #[test]
    fn base_paths_follow_the_platform_dirs() {
        let config = Path::new("/home/u/.config");
        let data = Path::new("/home/u/.local/share");
        assert_eq!(
            config_base_paths(Some(config), Some(data)),
            vec![
                config.join("JetBrains"),
                data.join("JetBrains"),
                config.join("Google"),
            ]
        );
        // macOS and Windows: config and data share one directory.
        let support = Path::new("/Users/u/Library/Application Support");
        assert_eq!(
            config_base_paths(Some(support), Some(support)),
            vec![support.join("JetBrains"), support.join("Google")]
        );
        assert_eq!(config_base_paths(None, None), Vec::<PathBuf>::new());
    }

    #[test]
    fn detects_ides_with_quota_files_sorted() {
        let root = tempfile::tempdir().unwrap();
        let jetbrains = root.path().join("JetBrains");
        let google = root.path().join("Google");
        add_ide(&jetbrains, "PyCharm2024.2", true);
        add_ide(&jetbrains, "PyCharm2025.1", true);
        add_ide(&jetbrains, "IntelliJIdea2025.3", true);
        add_ide(&jetbrains, "WebStorm2025.1", false);
        add_ide(&google, "AndroidStudio2024.2", true);
        std::fs::create_dir_all(jetbrains.join("consentOptions")).unwrap();

        let bases = vec![jetbrains, root.path().join("missing"), google];
        let names: Vec<String> = detect_installed_ides(&bases)
            .iter()
            .map(IdeInfo::display_name)
            .collect();
        assert_eq!(
            names,
            vec![
                "Android Studio 2024.2",
                "IntelliJ IDEA 2025.3",
                "PyCharm 2025.1",
                "PyCharm 2024.2",
            ]
        );
    }

    #[test]
    fn latest_ide_is_the_most_recently_written_quota_file() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("JetBrains");
        let idea = add_ide(&base, "IntelliJIdea2025.3", true);
        let pycharm = add_ide(&base, "PyCharm2024.2", true);
        set_modified(&idea, 1_700_000_000);
        set_modified(&pycharm, 1_750_000_000);
        let bases = vec![base];
        assert_eq!(
            detect_latest_ide(&bases).unwrap().display_name(),
            "PyCharm 2024.2"
        );
        set_modified(&idea, 1_760_000_000);
        assert_eq!(
            detect_latest_ide(&bases).unwrap().display_name(),
            "IntelliJ IDEA 2025.3"
        );
    }

    #[test]
    fn no_ides_detected() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(detect_latest_ide(&[root.path().to_path_buf()]), None);
        assert_eq!(detect_latest_ide(&[]), None);
    }
}
