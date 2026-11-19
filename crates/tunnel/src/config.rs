//! The part of `~/.ssh/config` a tunnel needs: for a host alias, its `HostName`, `Port`, `User`,
//! `IdentityFile` and `ProxyJump`. As in OpenSSH, the first value found for a keyword wins and
//! `Host` patterns may use `*`, `?` and `!`. `Include` is followed; `Match` blocks are skipped,
//! since their conditions are not evaluated.

use std::path::{Path, PathBuf};

/// What the config says about one host. Empty when the file is missing or names nothing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct HostSettings {
    pub host_name: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_files: Vec<String>,
    /// `None` when unset; `Some("none")` turns jumping off.
    pub proxy_jump: Option<String>,
}

/// The settings for `alias` in the config file at `path`.
pub(crate) fn lookup(path: &Path, alias: &str) -> HostSettings {
    let mut settings = HostSettings::default();
    let base = path.parent().map(Path::to_owned).unwrap_or_default();
    read_file(path, &base, alias, &mut settings, 0);
    if let Some(name) = &settings.host_name {
        settings.host_name = Some(name.replace("%h", alias));
    }
    settings
}

/// Nested `Include`s deeper than this are ignored, so a loop cannot hang.
const MAX_DEPTH: u8 = 8;

fn read_file(path: &Path, base: &Path, alias: &str, settings: &mut HostSettings, depth: u8) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    // Lines before the first `Host` apply to every host.
    let mut applies = true;
    for line in text.lines() {
        let Some((keyword, value)) = split(line) else {
            continue;
        };
        match keyword.as_str() {
            "host" => applies = host_matches(&value, alias),
            "match" => applies = false,
            "include" if applies => {
                for pattern in value.split_whitespace() {
                    for file in include_paths(base, pattern) {
                        read_file(&file, base, alias, settings, depth + 1);
                    }
                }
            }
            _ if applies => apply(settings, &keyword, &value),
            _ => {}
        }
    }
}

fn apply(settings: &mut HostSettings, keyword: &str, value: &str) {
    let first_word = || value.split_whitespace().next().map(unquote);
    match keyword {
        "hostname" if settings.host_name.is_none() => settings.host_name = first_word(),
        "port" if settings.port.is_none() => {
            settings.port = first_word().and_then(|p| p.parse().ok());
        }
        "user" if settings.user.is_none() => settings.user = first_word(),
        "identityfile" => settings.identity_files.extend(first_word()),
        "proxyjump" if settings.proxy_jump.is_none() => settings.proxy_jump = first_word(),
        _ => {}
    }
}

/// `Keyword value` or `Keyword=value`, keyword lowercased; `None` for blanks and comments.
fn split(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let (keyword, rest) = line.split_at(end);
    let value = rest.trim_start().strip_prefix('=').unwrap_or(rest).trim();
    Some((keyword.to_ascii_lowercase(), value.to_owned()))
}

fn unquote(word: &str) -> String {
    word.trim_matches('"').to_owned()
}

/// Whether a `Host` line's patterns select `alias`: one positive match and no negated one.
fn host_matches(patterns: &str, alias: &str) -> bool {
    let mut matched = false;
    for pattern in patterns.split_whitespace().map(unquote) {
        if let Some(negated) = pattern.strip_prefix('!') {
            if glob(negated, alias) {
                return false;
            }
        } else if glob(&pattern, alias) {
            matched = true;
        }
    }
    matched
}

/// `*` and `?` wildcards, compared without case as OpenSSH does for host names.
fn glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// The files an `Include` names: relative paths start in `~/.ssh`, and the file name may hold
/// wildcards.
fn include_paths(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let path = crate::expand_home(&unquote(pattern));
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_str().is_some_and(|n| glob(name, n)))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_config(text: &str, alias: &str) -> HostSettings {
        let dir = std::env::temp_dir().join(format!(
            "quarry-ssh-config-{}-{}",
            std::process::id(),
            alias.replace(['*', '/'], "_")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config");
        std::fs::write(&path, text).unwrap();
        let settings = lookup(&path, alias);
        std::fs::remove_dir_all(dir).unwrap();
        settings
    }

    #[test]
    fn reads_an_alias_and_the_first_value_wins() {
        let settings = with_config(
            "Host prod-db\n  HostName 10.0.0.5\n  Port 2222\n  User deploy\n  \
             IdentityFile ~/.ssh/prod\n  ProxyJump bastion\n\
             Host *\n  User everyone\n  IdentityFile ~/.ssh/id_ed25519\n",
            "prod-db",
        );
        assert_eq!(
            settings,
            HostSettings {
                host_name: Some("10.0.0.5".into()),
                port: Some(2222),
                user: Some("deploy".into()),
                identity_files: vec!["~/.ssh/prod".into(), "~/.ssh/id_ed25519".into()],
                proxy_jump: Some("bastion".into()),
            }
        );
    }

    #[test]
    fn host_patterns_wildcards_and_negation() {
        let config = "Host *.internal !secret.internal\n  User ops\nHost=db?\n  Port=2200\n";
        assert_eq!(
            with_config(config, "api.internal").user.as_deref(),
            Some("ops")
        );
        assert_eq!(with_config(config, "secret.internal").user, None);
        assert_eq!(with_config(config, "db1").port, Some(2200));
        assert_eq!(with_config(config, "db12").port, None);
    }

    #[test]
    fn hostname_token_and_match_blocks() {
        let config = "Match host x\n  User skipped\nHost *\n  HostName %h.example.com\n";
        let settings = with_config(config, "web");
        assert_eq!(settings.host_name.as_deref(), Some("web.example.com"));
        assert_eq!(settings.user, None);
    }

    #[test]
    fn follows_includes() {
        let dir = std::env::temp_dir().join(format!("quarry-ssh-include-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("conf.d")).unwrap();
        std::fs::write(dir.join("conf.d/work"), "Host work\n  HostName 192.0.2.1\n").unwrap();
        std::fs::write(dir.join("config"), "Include conf.d/*\n").unwrap();
        let settings = lookup(&dir.join("config"), "work");
        assert_eq!(settings.host_name.as_deref(), Some("192.0.2.1"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_file_says_nothing() {
        assert_eq!(
            lookup(Path::new("/nonexistent/quarry/config"), "x"),
            HostSettings::default()
        );
    }
}
