use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct GmodRepo {
    pub root_dir: PathBuf,
}

pub struct SourceSnapshot {
    pub revision: String,
    pub github_url: String,
}

impl SourceSnapshot {
    pub fn source_url(&self, file: &str, start: usize, end: usize) -> String {
        let normalized = file.replace('\\', "/");
        let normalized = normalized.trim_start_matches('/');
        let normalized = normalized.strip_prefix("garrysmod/").unwrap_or(normalized);
        let mut url = reqwest::Url::parse(&format!("{}/blob/", self.github_url)).unwrap();
        {
            let mut segments = url.path_segments_mut().unwrap();
            segments
                .pop_if_empty()
                .push(&self.revision)
                .push("garrysmod");
            for segment in normalized.split('/').filter(|segment| !segment.is_empty()) {
                segments.push(segment);
            }
        }
        url.set_fragment(Some(&if start == end {
            format!("L{}", start)
        } else {
            format!("L{}-L{}", start, end)
        }));
        url.into()
    }
}

impl GmodRepo {
    /// Read immutable blobs from the revision used by report links, never the worktree.
    /// Revisit this boundary if auditing uncommitted code becomes a supported mode;
    /// that mode must carry its own evidence rather than claiming commit provenance.
    pub fn snapshot_sources(
        &self,
        snapshot: &SourceSnapshot,
    ) -> Result<Vec<(String, Vec<u8>)>, String> {
        let listing = Command::new("git")
            .args([
                "ls-tree",
                "-r",
                "-z",
                &snapshot.revision,
                "--",
                "garrysmod/lua",
                "garrysmod/gamemodes",
            ])
            .current_dir(&self.root_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !listing.status.success() {
            return Err("Could not enumerate committed Lua sources".into());
        }
        let mut files = Vec::new();
        for record in listing
            .stdout
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            let record = std::str::from_utf8(record).map_err(|_| "Source path is not UTF-8")?;
            let (metadata, path) = record.split_once('\t').ok_or("Invalid Git tree record")?;
            if !path.ends_with(".lua") {
                continue;
            }
            let fields: Vec<_> = metadata.split_whitespace().collect();
            if fields.len() != 3 || fields[1] != "blob" || !matches!(fields[0], "100644" | "100755")
            {
                return Err(format!("Unsupported committed source entry: {path}"));
            }
            files.push((
                path.strip_prefix("garrysmod/").unwrap().to_string(),
                fields[2].to_string(),
            ));
        }
        if files.is_empty() {
            return Err("Source revision contains no Lua sources under garrysmod/lua or garrysmod/gamemodes".into());
        }
        let input = files
            .iter()
            .map(|(_, oid)| format!("{oid}\n"))
            .collect::<String>();
        let mut child = Command::new("git")
            .args(["cat-file", "--batch"])
            .current_dir(&self.root_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("Git stdin unavailable")?;
        // Read stdout while feeding stdin: a large repository can fill both pipes.
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        writer
            .join()
            .map_err(|_| "Git input writer panicked")?
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("Could not read committed source blobs".into());
        }
        let mut remaining = output.stdout.as_slice();
        let mut sources = Vec::new();
        for (path, oid) in files {
            let end = remaining
                .iter()
                .position(|byte| *byte == b'\n')
                .ok_or("Truncated Git blob header")?;
            let header =
                std::str::from_utf8(&remaining[..end]).map_err(|_| "Invalid Git blob header")?;
            let fields: Vec<_> = header.split_whitespace().collect();
            if fields.len() != 3 || fields[0] != oid || fields[1] != "blob" {
                return Err(format!("Missing source blob: {path}"));
            }
            let length = fields[2]
                .parse::<usize>()
                .map_err(|_| "Invalid Git blob length")?;
            remaining = &remaining[end + 1..];
            if remaining.len() <= length || remaining[length] != b'\n' {
                return Err("Truncated Git blob".into());
            }
            sources.push((path, remaining[..length].to_vec()));
            remaining = &remaining[length + 1..];
        }
        Ok(sources)
    }

    pub fn source_snapshot(&self) -> Result<SourceSnapshot, String> {
        let git_output = |args: &[&str]| -> Result<String, String> {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.root_dir)
                .output()
                .map_err(|error| format!("Failed to inspect source checkout: {}", error))?;
            if !output.status.success() {
                return Err("Could not read the source checkout's commit and GitHub remote".into());
            }
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        };
        let revision = git_output(&["rev-parse", "HEAD"])?;
        let remote = git_output(&["remote", "get-url", "origin"])?;
        let repository = remote
            .strip_prefix("https://github.com/")
            .or_else(|| remote.strip_prefix("git@github.com:"))
            .ok_or("The source checkout's origin is not a GitHub repository")?
            .trim_end_matches('/')
            .trim_end_matches(".git");
        if !revision.chars().all(|c| c.is_ascii_hexdigit()) || revision.len() < 40 {
            return Err("Invalid source checkout revision".into());
        }
        Ok(SourceSnapshot {
            revision,
            github_url: format!("https://github.com/{}", repository),
        })
    }
    /// Ensures the Garry's Mod repository is cloned and available at `target_path`.
    pub fn ensure_repo(target_path: &Path, auto_pull: bool) -> Result<Self, String> {
        if target_path.exists() && target_path.join(".git").exists() {
            if auto_pull {
                println!(
                    "Updating existing repository at {}...",
                    target_path.display()
                );
                let status = Command::new("git")
                    .args(["pull", "--ff-only"])
                    .current_dir(target_path)
                    .status()
                    .map_err(|e| format!("Failed to run git pull: {}", e))?;
                if !status.success() {
                    return Err("git pull failed; requested source update was not completed".into());
                }
            } else {
                println!("Using existing repository at {}", target_path.display());
            }
            Ok(Self {
                root_dir: target_path.to_path_buf(),
            })
        } else {
            println!(
                "Cloning Facepunch/garrysmod repository into {}...",
                target_path.display()
            );
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
            }

            let status = Command::new("git")
                .args([
                    "clone",
                    "--depth",
                    "1",
                    "https://github.com/Facepunch/garrysmod",
                ])
                .arg(target_path)
                .status()
                .map_err(|e| format!("Failed to run git clone: {}", e))?;

            if !status.success() {
                return Err(format!(
                    "git clone failed with exit code: {:?}",
                    status.code()
                ));
            }

            Ok(Self {
                root_dir: target_path.to_path_buf(),
            })
        }
    }

    /// Resolves a wiki file path (e.g. `lua/includes/util/color.lua`) to a local file.
    pub fn resolve_file_path(&self, relative_path: &str) -> Option<PathBuf> {
        let clean = normalized_source_path(relative_path)?;

        // Path variation 1: <root>/garrysmod/<relative_path>
        let root = self.root_dir.canonicalize().ok()?;
        for candidate in [
            self.root_dir.join("garrysmod").join(&clean),
            self.root_dir.join(&clean),
        ] {
            if candidate.is_file() && candidate.canonicalize().ok()?.starts_with(&root) {
                return Some(candidate);
            }
        }

        None
    }

    /// Normalizes an absolute path back to a repo-relative wiki path (e.g. `lua/includes/util/color.lua`).
    pub fn to_wiki_relative_path(&self, abs_path: &Path) -> Option<String> {
        let garrysmod_dir = self.root_dir.join("garrysmod");
        if let Ok(rel) = abs_path.strip_prefix(&garrysmod_dir) {
            return Some(rel.to_slash_lossy());
        }
        if let Ok(rel) = abs_path.strip_prefix(&self.root_dir) {
            return Some(rel.to_slash_lossy());
        }
        None
    }
}

/// Wiki paths are relative source identities, never filesystem instructions.
pub fn normalized_source_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let path = path
        .trim_start_matches('/')
        .strip_prefix("garrysmod/")
        .unwrap_or(path.trim_start_matches('/'));
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part.contains(':') => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub trait PathSlashExt {
    fn to_slash_lossy(&self) -> String;
}

impl PathSlashExt for Path {
    fn to_slash_lossy(&self) -> String {
        self.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_links_pin_the_commit_and_correct_single_or_multiple_lines() {
        let snapshot = SourceSnapshot {
            revision: "abc123".into(),
            github_url: "https://github.com/Facepunch/garrysmod".into(),
        };
        assert_eq!(
            snapshot.source_url("lua/vgui/dbinder.lua", 5, 5),
            "https://github.com/Facepunch/garrysmod/blob/abc123/garrysmod/lua/vgui/dbinder.lua#L5"
        );
        assert_eq!(
            snapshot.source_url(r"gamemodes\base\gamemode\cl_voice.lua", 36, 45),
            "https://github.com/Facepunch/garrysmod/blob/abc123/garrysmod/gamemodes/base/gamemode/cl_voice.lua#L36-L45"
        );
        assert_eq!(
            snapshot.source_url("garrysmod/lua/example file.lua", 2, 9),
            "https://github.com/Facepunch/garrysmod/blob/abc123/garrysmod/lua/example%20file.lua#L2-L9"
        );
    }
}
