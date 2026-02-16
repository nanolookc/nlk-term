use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    path: String,
    status: String,
    staged: bool,
    unstaged: bool,
    untracked: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResponse {
    repo_path: String,
    branch: String,
    ahead: usize,
    behind: usize,
    changes: Vec<GitChange>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchesResponse {
    current: String,
    branches: Vec<String>,
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        "git command failed".to_string()
    } else {
        stderr
    })
}

fn resolve_git_root(path: &Path) -> Result<PathBuf, String> {
    let output = run_git(path, &["rev-parse", "--show-toplevel"])?;
    let root = output.trim();
    if root.is_empty() {
        return Err("failed to detect git root".to_string());
    }
    Ok(PathBuf::from(root))
}

fn detect_repo_root(explicit_path: Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = explicit_path {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return resolve_git_root(&candidate);
        }
        return Err("repo path does not exist".to_string());
    }

    let mut candidate = std::env::current_dir().map_err(|error| format!("cwd error: {error}"))?;

    loop {
        if let Ok(root) = resolve_git_root(&candidate) {
            return Ok(root);
        }

        if !candidate.pop() {
            break;
        }
    }

    Err("git repository not found".to_string())
}

#[tauri::command]
pub fn git_status(repo_path: Option<String>) -> Result<GitStatusResponse, String> {
    let repo = detect_repo_root(repo_path)?;
    let raw = run_git(&repo, &["status", "--porcelain=v1", "--branch"])?;

    let mut branch = "unknown".to_string();
    let mut ahead: usize = 0;
    let mut behind: usize = 0;
    let mut changes = Vec::new();

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let mut head = rest.trim();
            let mut tracking_meta: Option<&str> = None;

            if let Some((prefix, suffix)) = rest.split_once(" [") {
                head = prefix.trim();
                tracking_meta = Some(suffix.trim_end_matches(']').trim());
            }

            branch = head
                .split_once("...")
                .map(|(left, _)| left)
                .unwrap_or(head)
                .trim()
                .to_string();

            if let Some(meta) = tracking_meta {
                for part in meta.split(',') {
                    let chunk = part.trim();
                    if let Some(value) = chunk.strip_prefix("ahead ") {
                        ahead = value.trim().parse::<usize>().unwrap_or(0);
                    } else if let Some(value) = chunk.strip_prefix("behind ") {
                        behind = value.trim().parse::<usize>().unwrap_or(0);
                    }
                }
            }
            continue;
        }

        if line.len() < 4 {
            continue;
        }

        let status = &line[0..2];
        let x = status.chars().next().unwrap_or(' ');
        let y = status.chars().nth(1).unwrap_or(' ');
        let mut path = line[3..].trim().to_string();

        if let Some((_, to)) = path.split_once(" -> ") {
            path = to.to_string();
        }

        changes.push(GitChange {
            path,
            status: status.to_string(),
            staged: x != ' ' && x != '?',
            unstaged: y != ' ',
            untracked: x == '?' && y == '?',
        });
    }

    Ok(GitStatusResponse {
        repo_path: repo.to_string_lossy().to_string(),
        branch,
        ahead,
        behind,
        changes,
    })
}

#[tauri::command]
pub fn git_diff(repo_path: String, path: String, staged: bool, untracked: bool) -> Result<String, String> {
    let repo = PathBuf::from(repo_path);

    if untracked {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["diff", "--no-index", "--", "/dev/null", path.as_str()])
            .output()
            .map_err(|error| format!("failed to run git diff: {error}"))?;

        // git diff --no-index returns exit code 1 when differences exist.
        if output.status.code() == Some(0) || output.status.code() == Some(1) {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "failed to generate diff".to_string()
        } else {
            stderr
        });
    }

    if staged {
        return run_git(&repo, &["diff", "--staged", "--", path.as_str()]);
    }

    run_git(&repo, &["diff", "--", path.as_str()])
}

#[tauri::command]
pub fn git_stage(repo_path: String, path: String) -> Result<(), String> {
    let repo = PathBuf::from(repo_path);
    run_git(&repo, &["add", "--", path.as_str()]).map(|_| ())
}

#[tauri::command]
pub fn git_stage_all(repo_path: String) -> Result<(), String> {
    let repo = PathBuf::from(repo_path);
    run_git(&repo, &["add", "--all"]).map(|_| ())
}

#[tauri::command]
pub fn git_unstage(repo_path: String, path: String) -> Result<(), String> {
    let repo = PathBuf::from(repo_path);

    if run_git(&repo, &["restore", "--staged", "--", path.as_str()]).is_ok() {
        return Ok(());
    }

    run_git(&repo, &["reset", "HEAD", "--", path.as_str()]).map(|_| ())
}

#[tauri::command]
pub fn git_commit(repo_path: String, message: String, amend: bool) -> Result<String, String> {
    let repo = PathBuf::from(repo_path);
    let trimmed = message.trim();

    if trimmed.is_empty() {
        return Err("commit message is empty".to_string());
    }

    let mut command = Command::new("git");
    command.arg("-C").arg(&repo).arg("commit").arg("-m").arg(trimmed);
    if amend {
        command.arg("--amend");
    }

    let output = command
        .output()
        .map_err(|error| format!("failed to run git commit: {error}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        "git commit failed".to_string()
    } else {
        stderr
    })
}

#[tauri::command]
pub fn git_fetch(repo_path: String) -> Result<String, String> {
    let repo = PathBuf::from(repo_path);
    run_git(&repo, &["fetch", "--prune"])
}

#[tauri::command]
pub fn git_pull(repo_path: String) -> Result<String, String> {
    let repo = PathBuf::from(repo_path);
    run_git(&repo, &["pull"])
}

#[tauri::command]
pub fn git_push(repo_path: String) -> Result<String, String> {
    let repo = PathBuf::from(repo_path);
    run_git(&repo, &["push"])
}

#[tauri::command]
pub fn git_branches(repo_path: String) -> Result<GitBranchesResponse, String> {
    let repo = PathBuf::from(repo_path);
    let current = run_git(&repo, &["branch", "--show-current"])?.trim().to_string();
    let raw = run_git(&repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;

    let mut branches = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>();
    branches.sort();

    Ok(GitBranchesResponse { current, branches })
}

#[tauri::command]
pub fn git_checkout(repo_path: String, branch: String) -> Result<String, String> {
    let repo = PathBuf::from(repo_path);
    let target = branch.trim().to_string();
    if target.is_empty() {
        return Err("branch name is empty".to_string());
    }

    if run_git(&repo, &["switch", target.as_str()]).is_ok() {
        return Ok(format!("Switched to branch '{target}'"));
    }

    run_git(&repo, &["checkout", target.as_str()])
}
