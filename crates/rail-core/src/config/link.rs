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

/// The compiled size a pattern may reach. A pattern comes from the user's own config and
/// matching is linear in the subject either way, so the bound is only here to catch a
/// runaway: Unicode classes compile large, and `(\w+)-(\w+)` alone passes 64 KiB.
pub const MAX_PATTERN_SIZE_BYTES: usize = 1024 * 1024;

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
    /// The groups `url` fills. A match is judged on these alone: a group the template
    /// ignores says nothing about whether the match stands for a record.
    used: Vec<usize>,
}

/// One address a subject produced, with the rule that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub name: String,
    pub url: String,
}

impl Compiled {
    /// Every address `subject` produces under this rule, in the order the matches occur, at
    /// most [`MAX_MATCHES`]. A match the template cannot spell a record's address from is
    /// left out: one whose used groups are all empty, and one whose capture is a dot
    /// segment.
    pub fn matches(&self, subject: &str) -> Vec<Match> {
        let mut found = Vec::new();
        for caps in self.pattern.captures_iter(subject).take(MAX_MATCHES) {
            let group = |n: usize| caps.get(n).map_or("", |m| m.as_str());
            if !self.stands_for_a_record(group) {
                continue;
            }
            found.push(Match {
                name: self.name.clone(),
                url: fill(&self.url, group),
            });
        }
        found
    }

    /// Whether a match is worth offering, judged on the groups the template uses.
    ///
    /// Two ways it is not. Every used group empty means the match stands for no record and
    /// the template would resolve to the bare path — a pattern like `(a)|(b)` leaves the
    /// branch it did not take empty, so the groups the template ignores must not have a say.
    /// And a used group of `.` or `..` is a path segment wherever it lands: percent-encoding
    /// does not hide it, because a parser reads `%2e%2e` as `..` and climbs out of the path
    /// the rule chose.
    fn stands_for_a_record<'h>(&self, group: impl Fn(usize) -> &'h str) -> bool {
        self.used.iter().any(|&n| !group(n).is_empty())
            && !self.used.iter().any(|&n| matches!(group(n), "." | ".."))
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
    let mut used = placeholders(&action.url);
    used.sort_unstable();
    used.dedup();
    Ok(Compiled {
        name: action.name.clone(),
        pattern,
        url: action.url.clone(),
        used,
    })
}

/// Every address `subject` offers under `actions`, in the order the matches occur, each
/// address once.
///
/// The same address twice is one choice, whether a subject repeats an id (`#1 re #1`) or two
/// rules read it the same way: which rule found it is not a question about where to go, and
/// a picker that asks it twice is asking nothing.
pub fn choices(actions: &[Compiled], subject: &str) -> Vec<Match> {
    let mut found: Vec<Match> = Vec::new();
    for action in actions {
        for one in action.matches(subject) {
            if !found.iter().any(|seen| seen.url == one.url) {
                found.push(one);
            }
        }
    }
    found
}

/// Every rule of `config`, compiled. Call it once: compiling is the expensive half, and a
/// subject is matched on a keypress.
///
/// # Errors
/// The first rule that does not compile.
pub fn compile_all(config: &Config) -> Result<Vec<Compiled>, String> {
    config.link_actions.iter().map(compile).collect()
}

/// Two values a template is filled with to find out what a capture can reach. Both are
/// unreserved under [`CAPTURE`], so percent-encoding leaves them whole, and they share no
/// character, so an authority built from one cannot equal one built from the other.
const PROBES: [&str; 2] = ["aaaaaaaa", "zzzzzzzz"];

/// Everything `template` settles before its path — the scheme, any credentials, the host and
/// the port — once every placeholder holds `probe`.
///
/// Reading the filled template with the parser that later opens the link is the point: a
/// check of its own devising can disagree with what the address turns out to mean. Looking
/// for a spelled-out host is not enough, because `https:///{1}` and `https://\/{1}` both
/// look like they name one and a parser reads the capture as the host in each.
///
/// # Errors
/// A template that does not parse, or whose scheme or host could never open.
fn settled_head(template: &str, probe: &str) -> Result<String, String> {
    let filled = fill(template, |_| probe);
    let url = url::Url::parse(&filled).map_err(|err| format!("url does not parse: {err}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("url must start with http:// or https://".to_string());
    }
    // http and https are special schemes, for which the parser refuses an empty host
    // itself, so this guard only holds if that ever stops being true. The address it
    // protects is worth the line.
    if url.host_str().is_none_or(str::is_empty) {
        return Err("url names no host".to_string());
    }
    Ok(url[..url::Position::BeforePath].to_string())
}

/// Whether `template`'s path holds a `.` or `..` segment once its placeholders are emptied,
/// which is the state a match with an empty capture leaves it in.
///
/// It has to be read as text. A URL parser resolves dot segments while it parses, so the
/// filled address cannot be asked about them afterwards — by then it has already climbed.
fn path_climbs(template: &str) -> bool {
    let emptied = fill(template, |_| "");
    let authority = emptied
        .split_once("//")
        .map_or(emptied.as_str(), |(_, rest)| rest);
    let path = authority
        .split(['?', '#'])
        .next()
        .and_then(|head| head.split_once('/'))
        .map_or("", |(_, path)| path);
    // A parser reads a backslash as a separator, so a segment can end at either.
    path.split(['/', '\\'])
        .any(|segment| segment == "." || segment == "..")
}

/// Check one template: at least one placeholder the pattern can fill, an authority no
/// capture can move, and a path that stays where it is written.
fn check_url(action: &LinkAction, groups: usize) -> Result<(), String> {
    let name = &action.name;
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
    let [first, second] = PROBES.map(|probe| settled_head(&action.url, probe));
    let (first, second) = (
        first.map_err(|why| format!("link action {name:?}: {why}"))?,
        second.map_err(|why| format!("link action {name:?}: {why}"))?,
    );
    if first != second {
        return Err(format!(
            "link action {name:?}: url must name its scheme and host in full before the \
             first {{n}}; as written a match decides them, reaching {first} or {second}"
        ));
    }
    if path_climbs(&action.url) {
        return Err(format!(
            "link action {name:?}: url's path holds a '.' or '..' segment, which leaves the \
             path the rule names as soon as a capture is empty"
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
        // A dot segment is not something encoding can defuse: a parser resolves `%2e%2e`
        // back to `..` and climbs out of the path the rule chose. Such a match is dropped.
        let deep = compile(&rule(
            r"\[(.+)\]",
            "https://tracker.example/p/secret/{1}/view",
        ))
        .unwrap();
        for subject in ["[..]", "[.]"] {
            assert_eq!(deep.matches(subject), Vec::new(), "{subject}");
        }
    }

    /// The destination host comes from the config alone. A template that would let a capture
    /// reach it is refused before it can ever run — including the shapes that only a URL
    /// parser sees through: extra slashes and a backslash both leave the host to the capture,
    /// however settled the template reads.
    #[test]
    fn a_template_whose_host_is_not_settled_is_refused() {
        for url in [
            "https://{1}.example/t",
            "https://tracker.example{1}",
            "https:///{1}",
            r"https://\/{1}",
        ] {
            let err = validate_link_actions(&config(vec![rule(r"(\w+)(\d*)", url)])).unwrap_err();
            assert!(
                err.contains("must name its scheme and host in full"),
                "{url}: {err}"
            );
        }
        // These never open at all: an empty authority, and a space where the host goes.
        for url in ["https://?{1}", "https://exa mple/{1}"] {
            let err = validate_link_actions(&config(vec![rule(r"(\w+)(\d*)", url)])).unwrap_err();
            assert!(err.contains("does not parse"), "{url}: {err}");
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

    /// The property the two checks above exist for, over the shapes a template can take and
    /// the things a subject can say: once a rule validates, no capture moves its scheme or
    /// its host, and none leaves the path the rule chose.
    #[test]
    fn a_validated_template_keeps_its_address_whatever_a_capture_holds() {
        const TEMPLATES: &[&str] = &[
            "https://tracker.example/t/{1}",
            "https://tracker.example/?id={1}",
            "https://tracker.example/#{1}",
            "https://tracker.example/a/{1}/b",
            "https://user@tracker.example/t/{1}",
            "http://tracker.example/t/{1}",
            "https://tracker.example/{1}/{1}",
        ];
        const CAPTURES: &[&str] = &[
            "12345",
            "..",
            ".",
            "../..",
            "../../admin",
            "evil.example",
            "evil.example/x",
            "a?b",
            "a#b",
            "a@b",
            "%2e%2e",
            "%2F",
            r"\",
            "//evil.example",
            ":443",
            "x.example:8080",
            "",
            " ",
            "..;",
            "%00",
        ];
        // Templates whose two placeholders sit in segments of their own, so a match that
        // fills one and leaves the other empty is reached — the case a single group never
        // produces, and the one that matters next to a path the template itself writes.
        const PAIRS: &[&str] = &[
            "https://tracker.example/t/{1}/{2}",
            "https://tracker.example/{1}/x/{2}",
            "https://tracker.example/a/{1}/b/{2}/c",
            "https://tracker.example/t/{2}?from={1}",
            "https://tracker.example/t/{1}{2}",
        ];
        // `(x)?` is empty for every capture but `x`, which is what leaves one placeholder
        // filled and the other not.
        const CASES: &[(&str, &[&str])] = &[(r"\[(.*)\]", TEMPLATES), (r"\[(x)?(.*)\]", PAIRS)];
        for (pattern, templates) in CASES {
            for template in *templates {
                let make = |capture: &str| format!("[{capture}]");
                assert_eq!(
                    validate_link_actions(&config(vec![rule(pattern, template)])),
                    Ok(()),
                    "{template}"
                );
                let compiled = compile(&rule(pattern, template)).unwrap();
                let want = url::Url::parse(&compiled.matches(&make("1"))[0].url).unwrap();
                // The path the template spells out before its first capture. Whatever a
                // capture holds, the address has to stay under it; a climb shows up here,
                // because the parser resolved the dot segments while parsing.
                let after_authority = template
                    .split_once("//")
                    .and_then(|(_, rest)| rest.split_once('/'))
                    .map_or("", |(_, path)| path);
                let fixed = format!(
                    "/{}",
                    &after_authority[..after_authority
                        .find(['{', '?', '#'])
                        .unwrap_or(after_authority.len())]
                );
                for capture in CAPTURES {
                    for found in compiled.matches(&make(capture)) {
                        let got = url::Url::parse(&found.url)
                            .unwrap_or_else(|e| panic!("{template} + {capture:?}: {e}"));
                        assert_eq!(got.scheme(), want.scheme(), "{template} + {capture:?}");
                        assert_eq!(got.host_str(), want.host_str(), "{template} + {capture:?}");
                        assert!(
                            got.path().starts_with(&fixed),
                            "{template} + {capture:?} left its path: {} not under {fixed}",
                            got.path()
                        );
                    }
                }
            }
        }
    }

    /// Dropping a capture of `.` or `..` is not the whole of it: the dots can be the
    /// template's own, and then an empty capture next to them climbs just the same. Such a
    /// template is refused when the config loads, where it can still be read as text — by
    /// the time an address has been parsed, it has already climbed.
    #[test]
    fn a_template_whose_own_path_climbs_is_refused() {
        for url in [
            "https://e.example/t/..{1}/{2}",
            "https://e.example/t/.{1}/{2}",
            "https://e.example/a/{1}/../{2}",
            "https://e.example/a/{1}/./{2}",
            r"https://e.example/a/{1}\..\{2}",
        ] {
            let err = validate_link_actions(&config(vec![rule(r"(x)?(y)", url)])).unwrap_err();
            assert!(err.contains("'.' or '..' segment"), "{url}: {err}");
        }
        // A dot inside a segment is not a dot segment, and neither is an empty one.
        for url in [
            "https://e.example/t/{1}.html",
            "https://e.example/t/x{1}/{2}",
            "https://e.example/t/{1}/{2}",
        ] {
            assert_eq!(
                validate_link_actions(&config(vec![rule(r"(x)?(y)", url)])),
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
        for url in ["file:///etc/{1}", "{1}://tracker.example/t/{2}"] {
            let err = validate_link_actions(&config(vec![rule(r"(\w+)(\d*)", url)])).unwrap_err();
            assert!(err.contains("http://"), "{url}: {err}");
        }
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

    /// An alternation leaves the branch it did not take empty. Only the groups the template
    /// fills decide whether a match stands for a record: judged on all of them, the subject
    /// `a` would look like a match for `{2}` and open the bare path.
    #[test]
    fn an_empty_group_the_template_uses_drops_the_match() {
        let second = compile(&rule(r"(a)|(b)", "https://e.example/t/{2}")).unwrap();
        assert_eq!(second.matches("a"), Vec::new());
        assert_eq!(second.matches("b")[0].url, "https://e.example/t/b");

        // The mirror image: a group the template ignores is no reason to drop a match.
        let first = compile(&rule(r"(a)|(b)", "https://e.example/t/{1}")).unwrap();
        assert_eq!(first.matches("a")[0].url, "https://e.example/t/a");
    }

    /// Unicode classes compile large, and the cap is only here to catch a runaway. These are
    /// the patterns a reader writes without a second thought; none may be turned away.
    #[test]
    fn an_ordinary_pattern_fits_the_compiled_size_cap() {
        for pattern in [r"(\w+)-(\w+)", r"\b(\w{1,20})\b", r"(?i)([\p{L}\d_-]+)"] {
            assert!(
                compile(&rule(pattern, "https://e.example/t/{1}")).is_ok(),
                "{pattern}"
            );
        }
    }

    /// The table as a reader writes it: `match` is the TOML key, `pattern` only the field it
    /// deserialises into, and the loader is what has to agree with the documentation.
    #[test]
    fn a_link_actions_table_parses_from_toml() {
        let text = r#"
            [[link_actions]]
            name = "tracker"
            match = '\(#(\d+)\)'
            url = "https://tracker.example/t/{1}"
        "#;
        let config = super::super::load::parse(text, std::path::Path::new("config.toml")).unwrap();
        assert_eq!(config.link_actions.len(), 1);
        assert_eq!(config.link_actions[0].pattern, r"\(#(\d+)\)");
        assert_eq!(
            compile(&config.link_actions[0])
                .unwrap()
                .matches("Fire (#12345)")[0]
                .url,
            "https://tracker.example/t/12345"
        );

        // `pattern` is not the key; writing it is a typo the loader has to name.
        let wrong_key = text.replace("match =", "pattern =");
        assert!(
            super::super::load::parse(&wrong_key, std::path::Path::new("config.toml")).is_err()
        );
    }

    /// One address is one choice. A subject that names the same record twice, and two rules
    /// that read a subject the same way, both ask one question, not two — and the order the
    /// matches occur in survives the removal.
    #[test]
    fn one_address_is_offered_once() {
        let ticket = compile(&rule(r"#(\d+)", "https://tracker.example/t/{1}")).unwrap();
        let urls =
            |found: Vec<Match>| -> Vec<String> { found.into_iter().map(|m| m.url).collect() };

        assert_eq!(
            urls(choices(std::slice::from_ref(&ticket), "#1 re #1")),
            ["https://tracker.example/t/1"]
        );
        assert_eq!(
            urls(choices(
                std::slice::from_ref(&ticket),
                "#2 blocks #1 and #2"
            )),
            ["https://tracker.example/t/2", "https://tracker.example/t/1"]
        );

        let same = LinkAction {
            name: "other".to_string(),
            pattern: r"ticket (\d+)".to_string(),
            url: "https://tracker.example/t/{1}".to_string(),
        };
        let both = [ticket, compile(&same).unwrap()];
        assert_eq!(
            urls(choices(&both, "#7 is ticket 7")),
            ["https://tracker.example/t/7"]
        );
    }

    /// Braces that are not a placeholder are the template's own text, so a path holding one
    /// survives untouched, and a brace that is never closed ends the search rather than the
    /// program.
    #[test]
    fn braces_that_name_no_group_stay_as_they_are() {
        let compiled = compile(&rule(r"(\d+)", "https://e.example/{x}/{1}")).unwrap();
        assert_eq!(compiled.matches("42")[0].url, "https://e.example/{x}/42");

        // Unclosed: there is no placeholder to find, so the template has none to fill and
        // the loader turns it away rather than opening one address for every match.
        let err = validate_link_actions(&config(vec![rule(r"(\d+)", "https://e.example/{1")]))
            .unwrap_err();
        assert!(err.contains("no {1}"), "{err}");
        let raw = compile(&rule(r"(\d+)", "https://e.example/t/{1")).unwrap();
        assert_eq!(
            raw.matches("42"),
            Vec::new(),
            "nothing to fill, nothing to offer"
        );
    }

    /// A pattern is the one field a reader is likely to paste a mailbox's shape into, so the
    /// debug line stands its length in for it. Everything else is the config's own text.
    #[test]
    fn the_debug_line_keeps_the_pattern_to_itself() {
        let pattern = r"\(#SECRET-(\d+)\)";
        let shown = format!("{:?}", rule(pattern, "https://e.example/t/{1}"));
        assert!(!shown.contains("SECRET"), "{shown}");
        assert!(
            shown.contains(&format!("pattern_chars: {}", pattern.chars().count())),
            "{shown}"
        );
        assert!(shown.contains("https://e.example/t/{1}"), "{shown}");
    }
}
