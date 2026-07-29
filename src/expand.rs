use std::env;

/// Expand a leading `~` or `~/` to `$HOME`.
pub fn expand_tilde(s: &str) -> String {
    expand_tilde_with(s, &env::var("HOME").unwrap_or_default())
}

/// Expand all `~` occurrences and `$VAR` / `${VAR}` shell variable references
/// using the current process environment.  Mirrors the bash `expand_value` helper.
pub fn expand_value(s: &str) -> String {
    let home = env::var("HOME").unwrap_or_default();
    expand_value_with(s, &home, &|name| env::var(name).ok())
}

/// `expand_tilde` with `home` supplied, so the logic is testable without
/// reading the process environment.
fn expand_tilde_with(s: &str, home: &str) -> String {
    if s == "~" {
        home.to_owned()
    } else if let Some(rest) = s.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        s.to_owned()
    }
}

/// `expand_value` with `home` and the variable lookup supplied, so the logic is
/// testable without mutating the process environment (a data race under
/// parallel tests, and `set_var` is unsafe as of edition 2024).
fn expand_value_with(s: &str, home: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let replaced = s.replace('~', home);
    expand_shell_vars(&replaced, lookup)
}

fn expand_shell_vars(s: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '$' {
            result.push(c);
            continue;
        }

        let braced = chars.peek() == Some(&'{');
        let mut var_name = String::new();

        if braced {
            chars.next(); // consume '{'
            for ch in chars.by_ref() {
                if ch == '}' {
                    break;
                }
                var_name.push(ch);
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    var_name.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
        }

        if let Some(val) = lookup(&var_name) {
            result.push_str(&val);
        }
        // Unknown variables expand to empty string, matching bash `eval` behaviour
        // when the variable is unset.
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a variable lookup from a fixed set of pairs.
    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn tilde_alone() {
        assert_eq!(expand_tilde_with("~", "/home/user"), "/home/user");
    }

    #[test]
    fn tilde_prefix() {
        assert_eq!(expand_tilde_with("~/.cargo", "/home/user"), "/home/user/.cargo");
    }

    #[test]
    fn tilde_not_prefix_unchanged() {
        assert_eq!(expand_tilde_with("/usr/local/bin", "/home/user"), "/usr/local/bin");
    }

    #[test]
    fn expand_dollar_var() {
        let lookup = lookup_from(&[("_ORKA_TEST_VAR", "hello")]);
        assert_eq!(expand_shell_vars("$_ORKA_TEST_VAR/world", &lookup), "hello/world");
    }

    #[test]
    fn expand_braced_var() {
        let lookup = lookup_from(&[("_ORKA_TEST_BRACED", "hi")]);
        assert_eq!(expand_shell_vars("${_ORKA_TEST_BRACED}/there", &lookup), "hi/there");
    }

    #[test]
    fn expand_unknown_var_is_empty() {
        let lookup = lookup_from(&[]);
        assert_eq!(expand_shell_vars("$_ORKA_DEFINITELY_UNSET", &lookup), "");
    }

    #[test]
    fn expand_value_tilde_and_var() {
        let lookup = lookup_from(&[("_ORKA_TEST_SUFFIX", "bin")]);
        let result = expand_value_with("~/$_ORKA_TEST_SUFFIX", "/home/user", &lookup);
        assert_eq!(result, "/home/user/bin");
    }
}
