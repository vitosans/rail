//! `[[link_actions]]`: a pattern over a thread's subject and the address it stands for.
//!
//! Mail from an issue tracker carries the record's id in its subject — `(#12345)`,
//! `[PROJ-123]`, `#4711` — so the way to the record is a substitution, not a search. A rule
//! names a pattern and a URL template whose `{1}`, `{2}`, … are the pattern's capture groups.
//!
//! The subject is the sender's text, so a capture is never trusted: it is percent-encoded
//! before it reaches the template, and a template may not place a capture in its scheme or
//! host. The destination host comes from the config and from nowhere else.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::model::Config;

/// Characters of a link action's name.
pub const MAX_LINK_ACTION_NAME_CHARS: usize = 40;

/// How many `[[link_actions]]` tables Rail reads. The picker lists one line per match, so a
/// mailbox full of rules would bury the question it asks.
pub const MAX_LINK_ACTIONS: usize = 20;

/// The compiled size a pattern may reach. A pattern comes from the user, not from mail, but
/// the bound keeps a careless one from spending the memory of a mailbox on a regex.
pub const MAX_PATTERN_SIZE_BYTES: usize = 64 * 1024;

/// How many matches one subject may contribute. A pathological pattern (`(.?)`) matches at
/// every position; the picker stops being a question long before that.
pub const MAX_MATCHES: usize = 20;

/// Everything but RFC 3986's unreserved set, so a capture carries no delimiter into the
/// template: a subject holding `/`, `?`, `#` or `@` cannot reach past the path it lands in.
const CAPTURE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// One `[[link_actions]]` table: `name` labels it, `match` is a regular expression over the
/// subject, `url` the template its captures fill.
#[derive(Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinkAction {
    pub name: String,
    #[serde(rename = "match")]
    pub pattern: String,
    pub url: String,
}

impl fmt::Debug for LinkAction {
    /// The name and the template; the pattern's length stands in for the pattern, which is
    /// the one field a user is likely to paste a mailbox's shape into.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkAction")
            .field("name", &self.name)
            .field("pattern_chars", &self.pattern.chars().count())
            .field("url", &self.url)
            .finish()
    }
}

/// A rule with its pattern compiled, ready to run over a subject.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub name: String,
    pattern: Regex,
    url: String,
}

/// One address a subject produced, with the rule that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub name: String,
    pub url: String,
}

impl Compiled {
    /// Every address `subject` produces under this rule, in the order the matches occur, at
    /// most [`MAX_MATCHES`]. A match whose groups are all empty is left out: it stands for no
    /// record, and its template would resolve to the bare path.
    pub fn matches(&self, subject: &str) -> Vec<Match> {
        let mut found = Vec::new();
        for caps in self.pattern.captures_iter(subject).take(MAX_MATCHES) {
            if (1..caps.len()).all(|i| caps.get(i).is_none_or(|m| m.as_str().is_empty())) {
                continue;
            }
            found.push(Match {
                name: self.name.clone(),
                url: fill(&self.url, |group| {
                    caps.get(group).map(|m| m.as_str()).unwrap_or_default()
                }),
            });
        }
        found
    }
}

/// `template` with each `{n}` replaced by `group(n)`, percent-encoded. A `{n}` the pattern
/// does not have cannot reach here: the loader refuses such a template.
fn fill<'a>(template: &str, group: impl Fn(usize) -> &'a str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}').map(|i| start + i) else {
            break;
        };
        match rest[start + 1..end].parse::<usize>() {
            Ok(n) => out.extend(utf8_percent_encode(group(n), CAPTURE)),
            // Not a placeholder: `{` is an ordinary character of the template.
            Err(_) => out.push_str(&rest[start..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The `{n}` placeholders of `template`, in the order they appear.
fn placeholders(template: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}').map(|i| start + i) else {
            break;
        };
        if let Ok(n) = rest[start + 1..end].parse::<usize>() {
            found.push(n);
        }
        rest = &rest[end + 1..];
    }
    found
}

/// Compile `action`'s pattern.
///
/// # Errors
/// The regex crate's message, prefixed with the rule's name, when the pattern does not
/// compile or exceeds [`MAX_PATTERN_SIZE_BYTES`] once compiled.
pub fn compile(action: &LinkAction) -> Result<Compiled, String> {
    let pattern = RegexBuilder::new(&action.pattern)
        .size_limit(MAX_PATTERN_SIZE_BYTES)
        .build()
        .map_err(|err| {
            format!(
                "link action {:?}: {}",
                action.name,
                err.to_string().replace('\n', " ")
            )
        })?;
    Ok(Compiled {
        name: action.name.clone(),
        pattern,
        url: action.url.clone(),
    })
}

/// Every rule of `config`, compiled. Call it once: compiling is the expensive half, and a
/// subject is matched on a keypress.
///
/// # Errors
/// The first rule that does not compile.
pub fn compile_all(config: &Config) -> Result<Vec<Compiled>, String> {
    config.link_actions.iter().map(compile).collect()
}

/// The template's text before its first placeholder, which is where its scheme and host have
/// to be complete.
fn before_first_placeholder(template: &str) -> &str {
    template.find('{').map_or(template, |i| &template[..i])
}

/// Check one template: `http`/`https`, a host that no capture can reach, and at least one
/// placeholder the pattern can fill.
fn check_url(action: &LinkAction, groups: usize) -> Result<(), String> {
    let name = &action.name;
    let head = before_first_placeholder(&action.url);
    let Some(rest) = head
        .strip_prefix("https://")
        .or_else(|| head.strip_prefix("http://"))
    else {
        return Err(format!(
            "link action {name:?}: url must start with http:// or https://, and its scheme must not hold a {{n}}"
        ));
    };
    // The authority ends at the first of these; seeing one proves the host is spelled out in
    // the config and cannot be extended by a capture.
    if !rest.contains(['/', '?', '#']) {
        return Err(format!(
            "link action {name:?}: url must name its host in full before the first {{n}}, \
             followed by '/', '?' or '#'"
        ));
    }
    let used = placeholders(&action.url);
    if used.is_empty() {
        return Err(format!(
            "link action {name:?}: url has no {{1}}, so every match would open the same address"
        ));
    }
    if let Some(n) = used.iter().find(|&&n| n == 0 || n > groups) {
        return Err(format!(
            "link action {name:?}: url uses {{{n}}}, but the pattern has {groups} capture \
             group(s) numbered 1 upwards"
        ));
    }
    Ok(())
}

/// Check every `[[link_actions]]` table: the name rules of a saved search, a pattern that
/// compiles, and a template that keeps its host to itself.
///
/// # Errors
/// The first rule that is refused, worded for `ConfigError::Invalid`.
pub fn validate_link_actions(config: &Config) -> Result<(), String> {
    if config.link_actions.len() > MAX_LINK_ACTIONS {
        return Err(format!(
            "at most {MAX_LINK_ACTIONS} [[link_actions]] tables, found {}",
            config.link_actions.len()
        ));
    }
    let mut names = std::collections::HashSet::new();
    for action in &config.link_actions {
        super::saved::check_name(&action.name).map_err(|_| {
            format!(
                "link action name {:?}: {}",
                action.name,
                super::saved::name_rule()
            )
        })?;
        if !names.insert(action.name.to_ascii_lowercase()) {
            return Err(format!(
                "duplicate link action name {:?} (names are compared ignoring case)",
                action.name
            ));
        }
        let compiled = compile(action)?;
        check_url(action, compiled.pattern.captures_len().saturating_sub(1))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One rule, named `t`, over `pattern` with `url`.
    fn rule(pattern: &str, url: &str) -> LinkAction {
        LinkAction {
            name: "t".to_string(),
            pattern: pattern.to_string(),
            url: url.to_string(),
        }
    }

    /// A config holding `actions`, so the loader's checks can run over it.
    fn config(actions: Vec<LinkAction>) -> Config {
        Config {
            link_actions: actions,
            ..Config::default()
        }
    }

    /// The shape the feature exists for: an id in the subject becomes the record's address.
    #[test]
    fn a_subject_id_fills_the_template() {
        let compiled = compile(&rule(r"\(#(\d+)\)", "https://tracker.example/t/{1}")).unwrap();
        assert_eq!(
            compiled.matches("Printer is on fire (#12345)"),
            vec![Match {
                name: "t".to_string(),
                url: "https://tracker.example/t/12345".to_string(),
            }]
        );
    }

    /// Each match is offered, not each rule: a subject carrying two ids gives two addresses,
    /// in the order they occur, so the picker can ask which one was meant.
    #[test]
    fn two_ids_in_one_subject_give_two_addresses() {
        let compiled = compile(&rule(r"\(#(\d+)\)", "https://tracker.example/t/{1}")).unwrap();
        let urls: Vec<_> = compiled
            .matches("Merge (#12345) into (#678)")
            .into_iter()
            .map(|m| m.url)
            .collect();
        assert_eq!(
            urls,
            [
                "https://tracker.example/t/12345",
                "https://tracker.example/t/678"
            ]
        );
    }

    /// A subject is the sender's text. A capture that holds a delimiter must not be able to
    /// steer the address: every one of these would otherwise leave the path the rule picked.
    #[test]
    fn a_capture_cannot_carry_a_delimiter_into_the_address() {
        let compiled = compile(&rule(r"\[(.+)\]", "https://tracker.example/t/{1}")).unwrap();
        for (subject, expected) in [
            ("[../../admin]", "https://tracker.example/t/..%2F..%2Fadmin"),
            ("[a?b=c]", "https://tracker.example/t/a%3Fb%3Dc"),
            ("[a#frag]", "https://tracker.example/t/a%23frag"),
            (
                "[evil.example@x]",
                "https://tracker.example/t/evil.example%40x",
            ),
        ] {
            assert_eq!(compiled.matches(subject)[0].url, expected, "{subject}");
        }
    }

    /// The destination host comes from the config alone. A template that would let a capture
    /// reach the scheme or the host is refused before it can ever run.
    #[test]
    fn a_template_whose_host_is_not_settled_is_refused() {
        for url in [
            "https://{1}.example/t",
            "{1}://tracker.example/t/{2}",
            "https://tracker.example{1}",
        ] {
            let err = validate_link_actions(&config(vec![rule(r"(\w+)(\d*)", url)])).unwrap_err();
            assert!(err.contains("link action \"t\""), "{url}: {err}");
        }
        // Settled by a path, a query or a fragment: all three end the authority.
        for url in [
            "https://tracker.example/t/{1}",
            "https://tracker.example/?id={1}",
            "https://tracker.example/#{1}",
        ] {
            assert_eq!(
                validate_link_actions(&config(vec![rule(r"(\d+)", url)])),
                Ok(()),
                "{url}"
            );
        }
    }

    /// A template has to use what the pattern captures: without a placeholder every match
    /// would open one address, and a placeholder the pattern cannot fill is a typo.
    #[test]
    fn a_template_must_use_a_group_the_pattern_has() {
        let no_placeholder =
            validate_link_actions(&config(vec![rule(r"(\d+)", "https://tracker.example/t")]))
                .unwrap_err();
        assert!(no_placeholder.contains("no {1}"), "{no_placeholder}");

        let too_high = validate_link_actions(&config(vec![rule(
            r"(\d+)",
            "https://tracker.example/t/{2}",
        )]))
        .unwrap_err();
        assert!(too_high.contains("{2}"), "{too_high}");
    }

    /// Only `http` and `https`: Rail opens nothing else, and a template may not be the way
    /// round that.
    #[test]
    fn a_template_that_is_not_http_is_refused() {
        let err =
            validate_link_actions(&config(vec![rule(r"(\d+)", "file:///etc/{1}")])).unwrap_err();
        assert!(err.contains("http://"), "{err}");
    }

    /// A pattern that does not compile is named with its rule, so the message says which
    /// table to look at.
    #[test]
    fn a_pattern_that_does_not_compile_names_its_rule() {
        let err =
            validate_link_actions(&config(vec![rule("(", "https://e.example/{1}")])).unwrap_err();
        assert!(err.starts_with("link action \"t\": "), "{err}");
    }

    /// The name rules and the cap are a saved search's, so one wording covers both.
    #[test]
    fn names_follow_the_saved_search_rules_and_the_cap_holds() {
        let bad_charset = config(vec![LinkAction {
            name: "has space".to_string(),
            pattern: r"(\d+)".to_string(),
            url: "https://e.example/{1}".to_string(),
        }]);
        assert!(validate_link_actions(&bad_charset).is_err());

        let twice = config(vec![
            rule(r"(\d+)", "https://e.example/{1}"),
            rule(r"(\d+)", "https://e.example/{1}"),
        ]);
        assert!(
            validate_link_actions(&twice)
                .unwrap_err()
                .contains("duplicate")
        );

        let over_cap: Vec<_> = (0..=MAX_LINK_ACTIONS)
            .map(|n| LinkAction {
                name: format!("r{n}"),
                pattern: r"(\d+)".to_string(),
                url: "https://e.example/{1}".to_string(),
            })
            .collect();
        assert!(
            validate_link_actions(&config(over_cap))
                .unwrap_err()
                .contains("at most")
        );
    }

    /// A match whose groups are all empty stands for no record; opening the bare path would
    /// be worse than saying nothing matched.
    #[test]
    fn a_match_that_captures_nothing_is_left_out() {
        let compiled = compile(&rule(r"#(\d*)", "https://tracker.example/t/{1}")).unwrap();
        assert_eq!(compiled.matches("no id here: #"), Vec::new());
    }

    /// Braces that are not a placeholder are the template's own text, so a path holding one
    /// survives untouched.
    #[test]
    fn braces_that_name_no_group_stay_as_they_are() {
        let compiled = compile(&rule(r"(\d+)", "https://e.example/{x}/{1}")).unwrap();
        assert_eq!(compiled.matches("42")[0].url, "https://e.example/{x}/42");
    }
}
