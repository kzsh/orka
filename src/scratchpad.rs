//! Named persistent scratch directories and the interactive picker used when
//! no name is supplied on the command line.
//!
//! Scratchpads live under `$XDG_DATA_HOME/orka/scratch/<name>` (falling back to
//! `$HOME/.local/share`).  The picker is a self-contained fuzzy selector driven
//! through `/dev/tty` in raw mode; it deliberately avoids a TUI dependency.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

/// Root directory holding every scratchpad.
///
/// `xdg_data_home` is passed in rather than read from the environment so the
/// resolution logic is testable without mutating process-global state (a data
/// race under parallel tests, and unsafe as of edition 2024).
pub fn root(home: &str, xdg_data_home: Option<&str>) -> String {
    let data_home = xdg_data_home
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{home}/.local/share"));
    format!("{data_home}/orka/scratch")
}

/// Resolve and create (if necessary) the named scratchpad directory.
pub fn dir(name: &str, home: &str, xdg_data_home: Option<&str>) -> Result<String, String> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(format!(
            "invalid scratchpad name: {name:?} (must not be empty or contain '/')"
        ));
    }
    let path = format!("{}/{name}", root(home, xdg_data_home));
    fs::create_dir_all(&path)
        .map_err(|e| format!("failed to create scratchpad directory {path}: {e}"))?;
    Ok(path)
}

/// Names of existing scratchpads, sorted.  A missing root directory yields an
/// empty list rather than an error.
pub fn list(home: &str, xdg_data_home: Option<&str>) -> Result<Vec<String>, String> {
    let root = root(home, xdg_data_home);
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("failed to read {root}: {e}")),
    };

    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read {root}: {e}"))?;
        if entry.path().is_dir() {
            names.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    names.sort_unstable();
    Ok(names)
}

/// Score `candidate` against `query` as a fuzzy subsequence match.
///
/// Returns `None` when the query characters do not appear in order.  Higher
/// scores are better: consecutive runs and matches at word boundaries are
/// rewarded, skipped characters and long candidates are penalised.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let c: Vec<char> = candidate.to_lowercase().chars().collect();

    if q.is_empty() {
        return Some(0);
    }

    let mut score = 0i32;
    let mut qi = 0usize;
    let mut last_match: Option<usize> = None;

    for (ci, ch) in c.iter().enumerate() {
        if qi >= q.len() || *ch != q[qi] {
            continue;
        }
        if last_match == Some(ci.wrapping_sub(1)) {
            score += 8;
        }
        let boundary = ci == 0 || matches!(c[ci - 1], '-' | '_' | '.' | ' ' | '/');
        if boundary {
            score += 6;
        }
        score -= (ci - last_match.map_or(0, |l| l + 1)) as i32;
        last_match = Some(ci);
        qi += 1;
    }

    if qi != q.len() {
        return None;
    }
    score -= (c.len() - q.len()) as i32 / 4;
    Some(score)
}

/// Filter and rank `candidates` by `query`.  Ties keep the input order.
pub fn filter<'a>(query: &str, candidates: &'a [String]) -> Vec<&'a String> {
    let mut scored: Vec<(i32, usize, &String)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| fuzzy_score(query, c).map(|s| (s, i, c)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, c)| c).collect()
}

const MAX_VISIBLE: usize = 12;

/// Present an interactive fuzzy selector over `candidates`.
///
/// Returns `Ok(None)` when the user aborts (Esc or Ctrl-C).  Requires a
/// controlling terminal; without one the caller must supply a name explicitly.
pub fn pick(candidates: &[String], prompt: &str) -> Result<Option<String>, String> {
    let tty = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| format!("no terminal available for interactive selection: {e}"))?;

    let mut input = tty.try_clone().map_err(|e| e.to_string())?;
    let mut out = tty;
    // Declared last so it is dropped first: the terminal must be restored
    // while the file descriptor is still open.
    let _raw = RawMode::enable(out.as_raw_fd())?;

    let mut query = String::new();
    let mut cursor = 0usize;
    let mut drawn = 0usize;
    let mut byte = [0u8; 1];

    loop {
        let matches = filter(&query, candidates);
        cursor = cursor.min(matches.len().saturating_sub(1));
        drawn = render(&mut out, prompt, &query, &matches, cursor, drawn)?;

        let n = input
            .read(&mut byte)
            .map_err(|e| format!("failed to read from terminal: {e}"))?;
        if n == 0 {
            clear(&mut out, drawn)?;
            return Ok(None);
        }

        match byte[0] {
            // Ctrl-C
            3 => {
                clear(&mut out, drawn)?;
                return Ok(None);
            }
            // Esc, alone or introducing an arrow key.  The rest of an escape
            // sequence arrives immediately, so a short poll tells them apart
            // without blocking on a bare Esc.
            27 => {
                let mut seq = [0u8; 2];
                if read_available(&mut input, &mut seq, 25) == 2 && seq[0] == b'[' {
                    match seq[1] {
                        b'A' => cursor = cursor.saturating_sub(1),
                        b'B' => cursor = (cursor + 1).min(matches.len().saturating_sub(1)),
                        _ => {}
                    }
                    continue;
                }
                clear(&mut out, drawn)?;
                return Ok(None);
            }
            // Enter
            13 | 10 => {
                let picked = matches.get(cursor).map(|s| (*s).clone());
                clear(&mut out, drawn)?;
                return Ok(picked);
            }
            // Ctrl-P / Ctrl-N
            16 => cursor = cursor.saturating_sub(1),
            14 => cursor = (cursor + 1).min(matches.len().saturating_sub(1)),
            // Ctrl-U
            21 => {
                query.clear();
                cursor = 0;
            }
            // Ctrl-W
            23 => {
                while query.ends_with(' ') {
                    query.pop();
                }
                while !query.is_empty() && !query.ends_with(' ') {
                    query.pop();
                }
                cursor = 0;
            }
            // Backspace / DEL
            8 | 127 => {
                query.pop();
                cursor = 0;
            }
            b @ 0x20..=0x7e => {
                query.push(b as char);
                cursor = 0;
            }
            _ => {}
        }
    }
}

/// Read up to `buf.len()` bytes that are already available, waiting at most
/// `timeout_ms` for the first one.  Returns the number of bytes read.
fn read_available(input: &mut fs::File, buf: &mut [u8], timeout_ms: i32) -> usize {
    let fd = input.as_raw_fd();
    let mut filled = 0;
    while filled < buf.len() {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is a valid, initialised pollfd for a live descriptor.
        let ready = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if ready <= 0 {
            break;
        }
        match input.read(&mut buf[filled..filled + 1]) {
            Ok(1) => filled += 1,
            _ => break,
        }
    }
    filled
}

/// Draw the query line plus the visible slice of matches.  Returns the number
/// of lines written so the next frame can erase them.
fn render(
    out: &mut fs::File,
    prompt: &str,
    query: &str,
    matches: &[&String],
    cursor: usize,
    drawn: usize,
) -> Result<usize, String> {
    let mut buf = String::new();
    if drawn > 0 {
        buf.push_str(&format!("\r\x1b[{drawn}A"));
    }

    buf.push_str(&format!("\r\x1b[K{prompt}{query}\n"));
    let mut lines = 1;

    let start = cursor.saturating_sub(MAX_VISIBLE - 1);
    for (i, name) in matches.iter().enumerate().skip(start).take(MAX_VISIBLE) {
        if i == cursor {
            buf.push_str(&format!("\r\x1b[K\x1b[7m> {name}\x1b[0m\n"));
        } else {
            buf.push_str(&format!("\r\x1b[K  {name}\n"));
        }
        lines += 1;
    }

    if matches.is_empty() {
        buf.push_str("\r\x1b[K  (no match)\n");
        lines += 1;
    }

    // Erase any lines left over from a taller previous frame.
    for _ in lines..drawn {
        buf.push_str("\r\x1b[K\n");
    }
    let total = lines.max(drawn);

    out.write_all(buf.as_bytes())
        .map_err(|e| format!("failed to write to terminal: {e}"))?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(total)
}

/// Erase the rendered frame and leave the cursor where drawing began.
fn clear(out: &mut fs::File, drawn: usize) -> Result<(), String> {
    if drawn == 0 {
        return Ok(());
    }
    let mut buf = format!("\r\x1b[{drawn}A");
    for _ in 0..drawn {
        buf.push_str("\r\x1b[K\n");
    }
    buf.push_str(&format!("\r\x1b[{drawn}A"));
    out.write_all(buf.as_bytes())
        .map_err(|e| format!("failed to write to terminal: {e}"))?;
    out.flush().map_err(|e| e.to_string())
}

/// Puts the terminal in raw mode for the lifetime of the value and restores
/// the previous settings on drop, including on error paths.
struct RawMode {
    fd: std::os::fd::RawFd,
    original: libc::termios,
}

impl RawMode {
    fn enable(fd: std::os::fd::RawFd) -> Result<Self, String> {
        // SAFETY: `fd` is a live file descriptor for /dev/tty; termios is
        // fully initialised by tcgetattr before use.
        unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut original) != 0 {
                return Err("failed to read terminal attributes".to_string());
            }
            let mut raw = original;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                return Err("failed to set terminal to raw mode".to_string());
            }
            Ok(RawMode { fd, original })
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restoring the attributes captured in `enable`.
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_creates_directory() {
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap();
        let result = dir("test-pad", home, None).unwrap();
        assert_eq!(result, format!("{home}/.local/share/orka/scratch/test-pad"));
        assert!(std::path::Path::new(&result).is_dir());
    }

    #[test]
    fn dir_is_idempotent() {
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap();
        let r1 = dir("my-pad", home, None).unwrap();
        let r2 = dir("my-pad", home, None).unwrap();
        assert_eq!(r1, r2);
        assert!(std::path::Path::new(&r1).is_dir());
    }

    #[test]
    fn dir_honours_xdg_data_home() {
        let base = tempfile::tempdir().unwrap();
        let xdg = base.path().join("xdg");
        let path = dir("xpad", "/irrelevant", xdg.to_str()).unwrap();
        assert_eq!(path, format!("{}/orka/scratch/xpad", xdg.display()));
        assert!(std::path::Path::new(&path).is_dir());
    }

    #[test]
    fn dir_rejects_path_traversal() {
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap();
        assert!(dir("../evil", home, None).is_err());
        assert!(dir("a/b", home, None).is_err());
        assert!(dir("", home, None).is_err());
    }

    #[test]
    fn list_missing_root_is_empty() {
        let base = tempfile::tempdir().unwrap();
        let names = list(base.path().to_str().unwrap(), None).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn list_returns_sorted_directory_names() {
        let base = tempfile::tempdir().unwrap();
        let home = base.path().to_str().unwrap();
        dir("zeta", home, None).unwrap();
        dir("alpha", home, None).unwrap();
        dir("mid", home, None).unwrap();
        // A stray file in the root must not be listed.
        fs::write(format!("{}/note.txt", root(home, None)), "x").unwrap();

        let names = list(home, None).unwrap();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn fuzzy_score_requires_subsequence() {
        assert!(fuzzy_score("abc", "aXbXc").is_some());
        assert!(fuzzy_score("cba", "aXbXc").is_none());
        assert!(fuzzy_score("abcd", "abc").is_none());
    }

    #[test]
    fn fuzzy_score_is_case_insensitive() {
        assert_eq!(fuzzy_score("AB", "xaBz"), fuzzy_score("ab", "xabz"));
        assert_eq!(fuzzy_score("aB", "XAbZ"), fuzzy_score("ab", "xabz"));
        assert!(fuzzy_score("AB", "xyz").is_none());
    }

    #[test]
    fn fuzzy_score_empty_query_matches_everything() {
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn fuzzy_score_prefers_consecutive_and_boundary_matches() {
        let exact = fuzzy_score("rust", "rust").unwrap();
        let scattered = fuzzy_score("rust", "rXuXsXt").unwrap();
        assert!(exact > scattered);

        let boundary = fuzzy_score("t", "-t").unwrap();
        let interior = fuzzy_score("t", "xt").unwrap();
        assert!(boundary > interior);
    }

    #[test]
    fn filter_ranks_best_match_first() {
        let candidates = vec![
            "notes".to_string(),
            "network-tests".to_string(),
            "nt".to_string(),
            "zebra".to_string(),
        ];
        let got = filter("nt", &candidates);
        assert_eq!(*got[0], "nt");
        assert!(got.iter().all(|c| *c != "zebra"));
    }

    #[test]
    fn filter_empty_query_keeps_input_order() {
        let candidates = vec!["b".to_string(), "a".to_string()];
        let got: Vec<&str> = filter("", &candidates)
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(got, vec!["b", "a"]);
    }
}
