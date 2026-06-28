use std::env;

/// Expand a leading `~` or `~/` to `$HOME`.
pub fn expand_tilde(s: &str) -> String {
    let home = env::var("HOME").unwrap_or_default();
    if s == "~" {
        home
    } else if let Some(rest) = s.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        s.to_owned()
    }
}

/// Expand all `~` occurrences and `$VAR` / `${VAR}` shell variable references
/// using the current process environment.  Mirrors the bash `expand_value` helper.
pub fn expand_value(s: &str) -> String {
    let home = env::var("HOME").unwrap_or_default();
    let replaced = s.replace('~', &home);
    expand_shell_vars(&replaced)
}

fn expand_shell_vars(s: &str) -> String {
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

        if let Ok(val) = env::var(&var_name) {
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

    #[test]
    fn tilde_alone() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn tilde_prefix() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
        assert_eq!(expand_tilde("~/.cargo"), format!("{home}/.cargo"));
    }

    #[test]
    fn tilde_not_prefix_unchanged() {
        assert_eq!(expand_tilde("/usr/local/bin"), "/usr/local/bin");
    }

    #[test]
    fn expand_dollar_var() {
        std::env::set_var("_PITA_TEST_VAR", "hello");
        assert_eq!(expand_shell_vars("$_PITA_TEST_VAR/world"), "hello/world");
    }

    #[test]
    fn expand_braced_var() {
        std::env::set_var("_PITA_TEST_BRACED", "hi");
        assert_eq!(expand_shell_vars("${_PITA_TEST_BRACED}/there"), "hi/there");
    }

    #[test]
    fn expand_unknown_var_is_empty() {
        // Remove if set so we get the unset path.
        std::env::remove_var("_PITA_DEFINITELY_UNSET");
        assert_eq!(expand_shell_vars("$_PITA_DEFINITELY_UNSET"), "");
    }

    #[test]
    fn expand_value_tilde_and_var() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
        std::env::set_var("_PITA_TEST_SUFFIX", "bin");
        let result = expand_value("~/$_PITA_TEST_SUFFIX");
        assert_eq!(result, format!("{home}/bin"));
    }
}
