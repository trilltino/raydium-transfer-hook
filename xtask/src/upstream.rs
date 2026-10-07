use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Repositories that are cloned by default (the two Raydium programs we build and test against).
const DEFAULT_FETCH: &[&str] = &["cpmm", "clmm"];

/// Path prefixes/segments that would indicate copied Raydium source in this repository.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "vendor/",
    "third_party/raydium",
    "deps/raydium",
    "external/raydium",
    "fixtures/raydium-source",
];
const FORBIDDEN_SEGMENTS: &[&str] = &[
    "raydium-cp-swap",
    "raydium-clmm",
    "cp-swap",
    "raydium-sdk-V2",
];

/// Ignore rules that keep fetched upstream trees out of Git.
const REQUIRED_IGNORES: &[&str] = &["/target/", "/reference/"];

#[derive(Debug, Deserialize)]
struct Entry {
    repository: String,
    base_revision: String,
    hook_repository: Option<String>,
    hook_branch: Option<String>,
    hook_revision: Option<String>,
    package: Option<String>,
    program_dir: Option<String>,
}

type Lock = BTreeMap<String, Entry>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}

fn upstream_dir() -> PathBuf {
    std::env::var_os("RAYDIUM_HOOK_UPSTREAM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target").join("upstream"))
}

fn load_lock() -> Result<Lock> {
    let text = std::fs::read_to_string(root().join("upstream.lock.toml"))?;
    Ok(toml::from_str(&text)?)
}

fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(dir).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run(args: &[&str]) -> Result<()> {
    match args {
        ["list"] => list(),
        ["verify", flags @ ..] => verify(flags.contains(&"--offline")),
        ["fetch", rest @ ..] => {
            let hook = rest.contains(&"--hook");
            let locked = rest.contains(&"--locked");
            let names: Vec<&str> = rest.iter().copied().filter(|a| !a.starts_with("--")).collect();
            fetch(&names, hook, locked)
        }
        _ => Err("usage: cargo xtask upstream <verify [--offline] | fetch [NAME...] [--hook] [--locked] | list>".into()),
    }
}

fn list() -> Result<()> {
    for (name, entry) in load_lock()? {
        println!(
            "{name:<14} base {}  {}",
            entry.base_revision, entry.repository
        );
        if let Some(hook) = &entry.hook_revision {
            println!(
                "{:<14} hook {hook}  {}@{}",
                "",
                entry.hook_repository.as_deref().unwrap_or("?"),
                entry.hook_branch.as_deref().unwrap_or("?")
            );
        }
    }
    Ok(())
}

fn fetch(names: &[&str], hook: bool, locked: bool) -> Result<()> {
    let lock = load_lock()?;
    let names: Vec<&str> = if names.is_empty() {
        DEFAULT_FETCH.to_vec()
    } else {
        names.to_vec()
    };
    for name in names {
        let entry = lock
            .get(name)
            .ok_or_else(|| format!("`{name}` is not in upstream.lock.toml"))?;
        let (url, revision, dir_name) = if hook {
            let revision = entry
                .hook_revision
                .as_deref()
                .ok_or_else(|| format!("`{name}` has no hook_revision locked yet"))?;
            let url = entry
                .hook_repository
                .as_deref()
                .unwrap_or(&entry.repository);
            (url, revision, format!("{name}-hook"))
        } else {
            (
                entry.repository.as_str(),
                entry.base_revision.as_str(),
                name.to_string(),
            )
        };
        if !is_sha(revision) {
            return Err(format!(
                "`{name}` revision `{revision}` is not a full lowercase commit SHA"
            )
            .into());
        }
        let dir = upstream_dir().join(&dir_name);
        checkout(&dir, url, revision, locked)?;
        let lockfile = dir.join("Cargo.lock");
        println!(
            "{dir_name}: {revision}  ({})  upstream Cargo.lock {}",
            dir.display(),
            if lockfile.exists() {
                "present"
            } else {
                "absent"
            }
        );
        if let (Some(package), Some(program_dir)) = (&entry.package, &entry.program_dir) {
            println!("  build with the upstream toolchain: cargo build-sbf --manifest-path {}/{program_dir}/Cargo.toml  (package {package})", dir.display());
        }
    }
    Ok(())
}

fn checkout(dir: &Path, url: &str, revision: &str, locked: bool) -> Result<()> {
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(dir)?;
        git(dir, &["init", "-q"])?;
        git(dir, &["remote", "add", "origin", url])?;
    } else {
        if !git(dir, &["status", "--porcelain"])?.is_empty() {
            return Err(format!("{} has local modifications; refusing to touch it (delete the directory to refetch)", dir.display()).into());
        }
        let head = git(dir, &["rev-parse", "HEAD"])?;
        if head == revision {
            return Ok(());
        }
        if locked {
            return Err(format!(
                "{} is at unexpected commit {head}, locked revision is {revision}",
                dir.display()
            )
            .into());
        }
    }
    git(dir, &["fetch", "-q", "--depth", "1", "origin", revision])?;
    git(dir, &["checkout", "-q", "--detach", "FETCH_HEAD"])?;
    let head = git(dir, &["rev-parse", "HEAD"])?;
    if head != revision {
        return Err(format!("checked out {head}, expected {revision}").into());
    }
    Ok(())
}

fn verify(offline: bool) -> Result<()> {
    let lock = load_lock()?;
    let mut failures: Vec<String> = Vec::new();

    for (name, entry) in &lock {
        if !is_sha(&entry.base_revision) {
            failures.push(format!(
                "{name}: base_revision is not a 40-char lowercase SHA"
            ));
        }
        if !entry.repository.starts_with("https://github.com/") {
            failures.push(format!("{name}: repository must be an https GitHub URL"));
        }
        match (
            &entry.hook_repository,
            &entry.hook_branch,
            &entry.hook_revision,
        ) {
            (None, None, None) => {}
            (Some(_), Some(_), Some(revision)) => {
                if !is_sha(revision) {
                    failures.push(format!(
                        "{name}: hook_revision is not a 40-char lowercase SHA"
                    ));
                }
            }
            _ => failures.push(format!(
                "{name}: hook_repository, hook_branch and hook_revision must be set together"
            )),
        }
    }

    // No Raydium source may be tracked in this repository.
    let tracked = git(&root(), &["ls-files"])?;
    for path in tracked.lines() {
        let lower = path.to_ascii_lowercase();
        if FORBIDDEN_PREFIXES
            .iter()
            .any(|p| lower.starts_with(&p.to_ascii_lowercase()))
            || path.split('/').any(|seg| {
                FORBIDDEN_SEGMENTS
                    .iter()
                    .any(|f| seg.eq_ignore_ascii_case(f))
            })
            || path.ends_with("Anchor.toml")
        {
            failures.push(format!(
                "tracked path looks like copied Raydium source: {path}"
            ));
        }
    }

    let ignore = std::fs::read_to_string(root().join(".gitignore")).unwrap_or_default();
    for rule in REQUIRED_IGNORES {
        if !ignore.lines().any(|line| line.trim() == *rule) {
            failures.push(format!(
                ".gitignore is missing `{rule}` (fetched upstream trees must stay untracked)"
            ));
        }
    }

    if !offline {
        let cache = upstream_dir().join(".verify");
        for (name, entry) in &lock {
            let mut targets = vec![(
                entry.repository.as_str(),
                entry.base_revision.as_str(),
                "base",
            )];
            if let (Some(repo), Some(rev)) = (&entry.hook_repository, &entry.hook_revision) {
                targets.push((repo.as_str(), rev.as_str(), "hook"));
            }
            for (url, revision, kind) in targets {
                if !is_sha(revision) {
                    continue;
                }
                match remote_has_commit(&cache.join(format!("{name}-{kind}.git")), url, revision) {
                    Ok(()) => println!("{name} {kind}: {revision} present at {url}"),
                    Err(error) => failures.push(format!("{name} {kind}: {error}")),
                }
            }
        }
    }

    if failures.is_empty() {
        println!(
            "upstream lock OK{}",
            if offline {
                " (offline: remote revisions not checked)"
            } else {
                ""
            }
        );
        Ok(())
    } else {
        Err(format!(
            "upstream verification failed:\n  - {}",
            failures.join("\n  - ")
        )
        .into())
    }
}

/// Fetches exactly one commit into a bare cache (never a work tree) to prove it exists and
/// that the SHA resolves to itself.
fn remote_has_commit(cache: &Path, url: &str, revision: &str) -> Result<()> {
    if !cache.join("HEAD").exists() {
        std::fs::create_dir_all(cache)?;
        git(cache, &["init", "-q", "--bare"])?;
    }
    git(cache, &["fetch", "-q", "--depth", "1", url, revision])
        .map_err(|e| format!("cannot fetch {revision} from {url}: {e}"))?;
    let resolved = git(cache, &["rev-parse", "FETCH_HEAD"])?;
    if resolved != revision {
        return Err(format!("{url} resolved {revision} to {resolved}").into());
    }
    Ok(())
}
