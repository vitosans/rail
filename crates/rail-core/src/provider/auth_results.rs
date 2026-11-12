//! The receiving server's `Authentication-Results` (RFC 8601): which From domains it vouched
//! for. Only a `pass` that speaks for the From line counts: DMARC's `header.from`, or a DKIM
//! signature's domain (`header.d`, or the domain of `header.i`), which the sender must align
//! with (`aligned`). SPF checks the envelope sender, never the From line, so it vouches for
//! nothing. A sender can write its own `Authentication-Results` too, so callers read only the
//! topmost one, the receiving server's, and on Gmail only one that names `mx.google.com`.
//! Comments are dropped before anything is read, so a `(header.from=…)` inside one says
//! nothing.

/// The most domains kept from one header.
pub const MAX_VOUCHED: usize = 8;

/// The domains `header` vouches for, lower case, each once, in the order given. With
/// `authserv_id`, nothing unless the header names that server first.
pub fn vouched_domains(header: &str, authserv_id: Option<&str>) -> Vec<String> {
    let text = without_comments(header);
    let mut parts = text.split(';');
    if let Some(expected) = authserv_id {
        let named = parts.next().and_then(|p| p.split_whitespace().next());
        if !named.is_some_and(|name| name.eq_ignore_ascii_case(expected)) {
            return Vec::new();
        }
    }
    let mut domains: Vec<String> = Vec::new();
    for domain in parts.filter_map(vouched) {
        if domains.len() == MAX_VOUCHED {
            break;
        }
        if !domains.contains(&domain) {
            domains.push(domain);
        }
    }
    domains
}

/// Whether `address`'s domain is one of `vouched` or below one of them (relaxed alignment:
/// `eu.example.com` aligns with `example.com`, never the other way round).
pub fn aligned(address: &str, vouched: &[String]) -> bool {
    let Some((_, domain)) = address.rsplit_once('@') else {
        return false;
    };
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    vouched.iter().any(|v| {
        domain == *v
            || domain
                .strip_suffix(v.as_str())
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// `address`'s domain, lower case, when it has one shaped like a domain.
pub fn address_domain(address: &str) -> Option<String> {
    let (_, domain_part) = address.trim().rsplit_once('@')?;
    domain(domain_part)
}

/// The domain one result (`method=result prop=value …`) vouches for, if it passed.
fn vouched(result: &str) -> Option<String> {
    let mut tokens = result.split_whitespace();
    let (method, outcome) = tokens.next()?.split_once('=')?;
    if !outcome.eq_ignore_ascii_case("pass") {
        return None;
    }
    let method = method.split('/').next()?.to_ascii_lowercase();
    let wanted: &[&str] = match method.as_str() {
        "dmarc" => &["header.from"],
        "dkim" => &["header.d", "header.i"],
        _ => return None,
    };
    tokens.find_map(|token| {
        let (name, value) = token.split_once('=')?;
        wanted
            .iter()
            .any(|w| name.eq_ignore_ascii_case(w))
            .then(|| domain(value))?
    })
}

/// A property's value as a domain: unquoted, after any `@`, lower case, and only when it looks
/// like one (letters, digits, `-` and at least one inner `.`).
fn domain(value: &str) -> Option<String> {
    let value = value.trim_matches('"');
    let value = value.rsplit_once('@').map_or(value, |(_, d)| d);
    let value = value.trim_end_matches('.').to_ascii_lowercase();
    let shaped = value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    let dotted = value.contains('.') && !value.starts_with('.') && !value.contains("..");
    (shaped && dotted).then_some(value)
}

/// `header` without its comments (`(…)`, nested, `\` escaping the next character), each one a
/// space. Parentheses inside a quoted string are text.
fn without_comments(header: &str) -> String {
    let mut out = String::with_capacity(header.len());
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for c in header.chars() {
        if escaped {
            escaped = false;
            if depth == 0 {
                out.push(c);
            }
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' if depth == 0 => {
                quoted = !quoted;
                out.push(c);
            }
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth > 0 => {
                depth -= 1;
                out.push(' ');
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}
