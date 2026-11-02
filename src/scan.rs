use std::fs;
use std::path::{Path, PathBuf};

/// One tool's files on disk. `parsed` stays false until a reader prices them.
/// Copilot is not a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderScan {
    pub name: &'static str,
    pub located: bool,
    pub files: u64,
    pub parsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub providers: Vec<ProviderScan>,
}

#[derive(Debug, Clone)]
pub struct Roots {
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
    pub cursor_db: PathBuf,
}

impl Roots {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let claude_dir = std::env::var_os("CODEBURN_RS_CLAUDE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        let codex_dir = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let cursor_db = std::env::var_os("CODEBURN_RS_CURSOR_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let config = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"));
                config.join("Cursor/User/globalStorage/state.vscdb")
            });
        Self {
            claude_dir,
            codex_dir,
            cursor_db,
        }
    }
}

pub fn scan(roots: &Roots) -> Scan {
    let claude_projects = roots.claude_dir.join("projects");
    let codex_files = count_jsonl(&roots.codex_dir.join("sessions"))
        + count_jsonl(&roots.codex_dir.join("archived_sessions"));
    Scan {
        providers: vec![
            ProviderScan {
                name: "claude",
                located: roots.claude_dir.is_dir(),
                files: count_jsonl(&claude_projects),
                parsed: false,
            },
            ProviderScan {
                name: "codex",
                located: roots.codex_dir.is_dir(),
                files: codex_files,
                parsed: false,
            },
            ProviderScan {
                name: "cursor",
                located: roots.cursor_db.is_file(),
                files: 0,
                parsed: false,
            },
        ],
    }
}

/// Count `*.jsonl` files under `root`. Symlinks are not followed.
pub fn count_jsonl(root: &Path) -> u64 {
    fn walk(dir: &Path, total: &mut u64) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                walk(&path, total);
            } else if kind.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            {
                *total += 1;
            }
        }
    }

    let mut total = 0;
    if root.is_dir() {
        walk(root, &mut total);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codeburn-rs-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn counts_jsonl_only() {
        let dir = scratch("jsonl");
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/a.jsonl"), b"{}\n").unwrap();
        fs::write(dir.join("nested/b.txt"), b"no").unwrap();
        fs::write(dir.join("c.JSONL"), b"{}\n").unwrap();
        assert_eq!(count_jsonl(&dir), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_separates_the_three_homes() {
        let dir = scratch("roots");
        let claude = dir.join("claude");
        let codex = dir.join("codex");
        fs::create_dir_all(claude.join("projects/demo")).unwrap();
        fs::write(claude.join("projects/demo/s.jsonl"), b"{}\n").unwrap();
        fs::create_dir_all(codex.join("sessions/2026/11/02")).unwrap();
        fs::write(codex.join("sessions/2026/11/02/r.jsonl"), b"{}\n").unwrap();
        fs::create_dir_all(codex.join("archived_sessions")).unwrap();
        fs::write(codex.join("archived_sessions/old.jsonl"), b"{}\n").unwrap();
        let cursor = dir.join("state.vscdb");
        fs::write(&cursor, b"").unwrap();

        let found = scan(&Roots {
            claude_dir: claude,
            codex_dir: codex,
            cursor_db: cursor,
        });
        assert_eq!(
            found.providers,
            vec![
                ProviderScan {
                    name: "claude",
                    located: true,
                    files: 1,
                    parsed: false,
                },
                ProviderScan {
                    name: "codex",
                    located: true,
                    files: 2,
                    parsed: false,
                },
                ProviderScan {
                    name: "cursor",
                    located: true,
                    files: 0,
                    parsed: false,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
