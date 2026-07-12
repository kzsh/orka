use std::fs;
use std::path::Path;

use ignore::gitignore::GitignoreBuilder;
use tempfile::TempDir;
use walkdir::WalkDir;

/// Shadow volumes paired with the `TempDir` that holds their shared empty
/// source file.  `None` means no shadow configuration was found.
type ShadowResult = (Vec<(String, String)>, Option<TempDir>);

/// Scan every directory-typed volume in `volumes` for shadow patterns.
///
/// Patterns are loaded from two sources, applied in this order:
/// 1. `global_shadow` — `~/.config/orka/orkashadow`, applies to every mount.
/// 2. `.orkashadow` at the root of each mounted directory, applies to that
///    mount only.
///
/// For each file matched by either source, a shadow volume pair
/// `(empty_source, container_path)` is returned.
///
/// All shadow mounts share a single empty regular file as their host-side
/// source.  The caller must keep the returned `Option<TempDir>` alive until
/// the container exits; dropping it removes that source file.  `None` is
/// returned when no shadow configuration was found, and no temp dir is created.
///
/// Shadow volumes must be mounted `:ro` (read-only) so that writes from
/// inside the container are refused and never reach the host filesystem.
pub fn collect_shadow_volumes(
    volumes: &[(String, String)],
    global_shadow: &Path,
) -> Result<ShadowResult, String> {
    // Collect directory volumes — file mounts cannot have an .orkashadow.
    let dir_volumes: Vec<(&str, &str)> = volumes
        .iter()
        .filter(|(host, _)| Path::new(host.as_str()).is_dir())
        .map(|(h, c)| (h.as_str(), c.as_str()))
        .collect();

    if dir_volumes.is_empty() {
        return Ok((vec![], None));
    }

    // Skip all filesystem work when no shadow configuration exists anywhere.
    let any_shadow_exists = global_shadow.exists()
        || dir_volumes
            .iter()
            .any(|(host, _)| Path::new(host).join(".orkashadow").exists());

    if !any_shadow_exists {
        return Ok((vec![], None));
    }

    // One empty regular file is the source for every shadow mount.
    // It is a plain file (not a device node) so tools inside the container
    // that stat the path see a regular zero-byte file rather than a chardev.
    let tmp = tempfile::tempdir().map_err(|e| format!("failed to create shadow temp dir: {e}"))?;
    let empty = tmp.path().join("empty");
    fs::write(&empty, b"").map_err(|e| format!("failed to create shadow source file: {e}"))?;

    let mut shadows: Vec<(String, String)> = Vec::new();
    for (host, container) in &dir_volumes {
        shadows.extend(shadows_for_dir(host, container, &empty, global_shadow)?);
    }

    Ok((shadows, Some(tmp)))
}

fn shadows_for_dir(
    host_dir: &str,
    container_dir: &str,
    empty_source: &Path,
    global_shadow: &Path,
) -> Result<Vec<(String, String)>, String> {
    let local_shadow = Path::new(host_dir).join(".orkashadow");
    let has_global = global_shadow.exists();
    let has_local = local_shadow.exists();

    if !has_global && !has_local {
        return Ok(vec![]);
    }

    let mut builder = GitignoreBuilder::new(host_dir);
    // Global patterns are added first so per-repo patterns take precedence
    // (later entries win in gitignore semantics, including negation with !).
    if has_global {
        if let Some(err) = builder.add(global_shadow) {
            return Err(format!("{}: {err}", global_shadow.display()));
        }
    }
    if has_local {
        if let Some(err) = builder.add(&local_shadow) {
            return Err(format!("{host_dir}/.orkashadow: {err}"));
        }
    }
    let matcher = builder
        .build()
        .map_err(|e| format!("{host_dir}/.orkashadow: {e}"))?;

    let source = empty_source
        .to_str()
        .ok_or_else(|| "shadow temp path contains non-UTF-8 characters".to_string())?;

    let mut result = Vec::new();

    // Always shadow the .orkashadow file itself so the agent cannot read
    // which paths are being hidden from it.
    if has_local {
        let p = Path::new(container_dir).join(".orkashadow");
        result.push((source.to_string(), p.to_string_lossy().into_owned()));
    }

    for entry in WalkDir::new(host_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // matched_path_or_any_parents respects directory patterns (e.g. secrets/)
        // so files nested under a matched directory are caught too.
        if matcher.matched_path_or_any_parents(path, false).is_ignore() {
            let rel = path
                .strip_prefix(host_dir)
                .map_err(|e| format!("internal path strip error: {e}"))?;
            let container_path = Path::new(container_dir).join(rel);
            result.push((
                source.to_string(),
                container_path.to_string_lossy().into_owned(),
            ));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a temp directory pre-populated with `(relative_path, content)` pairs.
    fn make_dir(files: &[(&str, &str)]) -> TempDir {
        let dir = tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        dir
    }

    fn run(dir: &TempDir) -> Vec<(String, String)> {
        let host = dir.path().to_str().unwrap().to_string();
        let no_global = Path::new("/tmp/orka-no-such-global-shadow-xyzzy");
        let (shadows, _tmp) = collect_shadow_volumes(&[(host.clone(), host)], no_global).unwrap();
        shadows
    }

    #[test]
    fn no_orkashadow_returns_empty_and_no_tempdir() {
        let dir = make_dir(&[("main.rs", "fn main() {}")]);
        let host = dir.path().to_str().unwrap().to_string();
        let no_global = Path::new("/tmp/orka-no-such-global-shadow-xyzzy");
        let (shadows, tmp) = collect_shadow_volumes(&[(host.clone(), host)], no_global).unwrap();
        assert!(shadows.is_empty());
        assert!(
            tmp.is_none(),
            "no TempDir should be created when no shadow files exist"
        );
    }

    #[test]
    fn simple_filename_pattern() {
        let dir = make_dir(&[
            (".orkashadow", ".env\n"),
            (".env", "SECRET=abc"),
            ("main.rs", "fn main() {}"),
        ]);
        let shadows = run(&dir);
        assert_eq!(shadows.len(), 1);
        assert!(shadows[0].1.ends_with("/.env"));
    }

    #[test]
    fn wildcard_matches_multiple_files() {
        let dir = make_dir(&[
            (".orkashadow", "*.key\n"),
            ("private.key", "KEY"),
            ("server.key", "KEY"),
            ("readme.md", "docs"),
        ]);
        let shadows = run(&dir);
        assert_eq!(shadows.len(), 2);
        assert!(shadows.iter().all(|(_, c)| c.ends_with(".key")));
    }

    #[test]
    fn nested_directory_pattern() {
        let dir = make_dir(&[
            (".orkashadow", "secrets/\n"),
            ("secrets/api.key", "KEY"),
            ("secrets/token", "TOKEN"),
            ("main.rs", "fn main() {}"),
        ]);
        let shadows = run(&dir);
        assert_eq!(shadows.len(), 2);
    }

    #[test]
    fn globstar_pattern() {
        let dir = make_dir(&[
            (".orkashadow", "**/*.secret\n"),
            ("a/b/deep.secret", "x"),
            ("top.secret", "x"),
            ("main.rs", "fn main() {}"),
        ]);
        let shadows = run(&dir);
        assert_eq!(shadows.len(), 2);
    }

    #[test]
    fn shadow_source_is_empty_regular_file() {
        let dir = make_dir(&[(".orkashadow", ".env\n"), (".env", "SECRET=abc")]);
        let host = dir.path().to_str().unwrap().to_string();
        let no_global = Path::new("/tmp/orka-no-such-global-shadow-xyzzy");
        let (shadows, _tmp) = collect_shadow_volumes(&[(host.clone(), host)], no_global).unwrap();
        let (source, _) = &shadows[0];
        let meta = fs::metadata(source).unwrap();
        assert!(meta.is_file(), "shadow source must be a regular file");
        assert_eq!(meta.len(), 0, "shadow source must be empty");
    }

    #[test]
    fn file_volume_is_skipped() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("somefile.txt");
        fs::write(&file, "content").unwrap();
        let path = file.to_str().unwrap().to_string();
        let no_global = Path::new("/tmp/orka-no-such-global-shadow-xyzzy");
        let (shadows, _tmp) = collect_shadow_volumes(&[(path.clone(), path)], no_global).unwrap();
        assert!(shadows.is_empty());
    }

    #[test]
    fn global_shadow_applies_without_local() {
        let global_dir = tempdir().unwrap();
        let global_file = global_dir.path().join("orkashadow");
        fs::write(&global_file, ".env\n").unwrap();

        let dir = make_dir(&[(".env", "SECRET=abc"), ("main.rs", "fn main() {}")]);
        let host = dir.path().to_str().unwrap().to_string();
        let (shadows, _tmp) =
            collect_shadow_volumes(&[(host.clone(), host)], &global_file).unwrap();
        assert_eq!(shadows.len(), 1);
        assert!(shadows[0].1.ends_with("/.env"));
    }

    #[test]
    fn global_and_local_patterns_combine() {
        let global_dir = tempdir().unwrap();
        let global_file = global_dir.path().join("orkashadow");
        fs::write(&global_file, ".env\n").unwrap();

        let dir = make_dir(&[
            (".orkashadow", "*.key\n"),
            (".env", "SECRET=abc"),
            ("private.key", "KEY"),
            ("main.rs", "fn main() {}"),
        ]);
        let host = dir.path().to_str().unwrap().to_string();
        let (shadows, _tmp) =
            collect_shadow_volumes(&[(host.clone(), host)], &global_file).unwrap();
        assert_eq!(shadows.len(), 2);
        let container_paths: Vec<&str> = shadows.iter().map(|(_, c)| c.as_str()).collect();
        assert!(container_paths.iter().any(|p| p.ends_with("/.env")));
        assert!(container_paths.iter().any(|p| p.ends_with("/private.key")));
    }

    #[test]
    fn orkashadow_file_is_always_shadowed() {
        let dir = make_dir(&[
            (".orkashadow", ".env\n"),
            (".env", "SECRET=abc"),
            ("main.rs", "fn main() {}"),
        ]);
        let shadows = run(&dir);
        assert!(shadows.iter().any(|(_, c)| c.ends_with("/.orkashadow")));
    }

    #[test]
    fn orkashadow_shadowed_even_when_empty() {
        // An .orkashadow with no patterns (or only comments) still hides itself.
        let dir = make_dir(&[(".orkashadow", "# nothing\n"), ("main.rs", "fn main() {}")]);
        let shadows = run(&dir);
        assert_eq!(shadows.len(), 1);
        assert!(shadows[0].1.ends_with("/.orkashadow"));
    }

    #[test]
    fn comment_lines_are_ignored() {
        let dir = make_dir(&[
            (".orkashadow", "# this is a comment\n.env\n"),
            (".env", "SECRET=abc"),
            ("main.rs", "fn main() {}"),
        ]);
        let shadows = run(&dir);
        assert_eq!(shadows.len(), 1);
    }
}
