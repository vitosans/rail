//! HTML bodies as plain text, by Rail's own scanner. A browser parser builds a tree, and on
//! hostile mail its cost can grow faster than the mail (deep nesting, thousands of distinct
//! attributes, huge list numbers, indentation repeated on every line); this scanner builds
//! nothing. It reads each byte a constant number of times, keeps no stack and no lists,
//! fetches nothing and runs nothing:
//!
//! - text is copied, white space collapsed to one space;
//! - an `<a>` with an `href` Rail keeps (`provider::links::allowed`) becomes a `[n]` marker
//!   after its text, numbered by first appearance; the addresses come back with the text
//!   (`html_render`); a link closes only at the next `<a>`, at `</a>` or at the end of the
//!   text, so its marker always follows everything inside it, blocks included. The
//!   `<a>` tag is read once more, within its byte cap, to find `href`;
//! - tags are read only for their first few name bytes (block tags make line breaks, cells a
//!   space); attributes are skipped, quotes honoured only to find the tag's end, and a tag
//!   longer than `MAX_TAG_BYTES` ends there;
//! - comments are dropped, and so is the content of `script`, `style`, `head`, `title`,
//!   `template` and `noscript`; `head` ends, as in a browser, at the first text or start tag
//!   that cannot stand in it, so a missing `</head>` never hides the body;
//! - character references are decoded: numeric ones (never to a control character other than
//!   a line break or tab) and a small table of common names; anything else stays as written.
//!
//! The text is never longer than the HTML: the shortest anchor Rail numbers, `<a href=http:>`,
//! is 14 bytes and its marker at most `[200]`, 5 bytes.
//!
//! For the reader, `html_styled` gives the same text in runs of one style each: strong
//! (`b`, `strong`, `h1`-`h6`), emphasised (`i`, `em`), quoted (`blockquote`) and linked (a
//! numbered link's text and marker). Each style is a saturating count of open tags, not a
//! stack, so stray or missing end tags cost nothing; line breaks carry no style. Beside the
//! runs it lists the `<img>` tags it met (`image`), each standing before the run that follows
//! it; the text does not change.

mod image;
mod styled;

pub use image::{Image, MAX_ALT_CHARS, MAX_IMAGES};
pub use styled::{Run, Style, Styled, html_styled};

use crate::provider::links::{MAX_LINK_BYTES, Numbering, Rendered};

/// Bytes one tag (or `<!...>` declaration) may span before Rail treats it as closed.
pub(crate) const MAX_TAG_BYTES: usize = 4096;
/// The longest character reference name Rail looks up.
const MAX_ENTITY_NAME: usize = 32;
/// Digits of a numeric character reference (more cannot be a code point).
const MAX_DIGITS: usize = 8;
/// Tag name bytes compared; longer names match nothing.
const NAME_BYTES: usize = 10;
/// Consecutive line breaks kept.
const MAX_NEWLINES: usize = 2;

const BLOCK: &[&[u8]] = &[
    b"p",
    b"div",
    b"br",
    b"li",
    b"tr",
    b"h1",
    b"h2",
    b"h3",
    b"h4",
    b"h5",
    b"h6",
    b"blockquote",
    b"pre",
    b"table",
    b"ul",
    b"ol",
    b"dl",
    b"dt",
    b"dd",
    b"hr",
    b"section",
    b"article",
    b"header",
    b"footer",
];
const CELL: &[&[u8]] = &[b"td", b"th"];
const STRONG: &[&[u8]] = &[b"b", b"strong", b"h1", b"h2", b"h3", b"h4", b"h5", b"h6"];
const EMPHASIS: &[&[u8]] = &[b"i", b"em"];
const QUOTE: &[&[u8]] = &[b"blockquote"];
/// Elements whose content is dropped up to their own end tag.
const HIDDEN: &[&[u8]] = &[
    b"script",
    b"style",
    b"head",
    b"title",
    b"template",
    b"noscript",
];
/// Hidden elements that can stand inside `head` and hold text that looks like tags.
const HIDDEN_IN_HEAD: &[&[u8]] = &[b"script", b"style", b"title", b"template", b"noscript"];
/// Empty elements that can stand inside `head` without ending it.
const IN_HEAD: &[&[u8]] = &[b"meta", b"link", b"base"];

/// What one piece of markup inside `head` does.
enum HeadStep {
    /// `head` goes on from here.
    Inside(usize),
    /// `head` has ended; normal text resumes here.
    Leave(usize),
}

/// The character references Rail decodes by name.
const ENTITIES: &[(&[u8], char)] = &[
    (b"amp", '&'),
    (b"lt", '<'),
    (b"gt", '>'),
    (b"quot", '"'),
    (b"apos", '\''),
    (b"nbsp", '\u{a0}'),
    (b"copy", '©'),
    (b"reg", '®'),
    (b"trade", '™'),
    (b"hellip", '…'),
    (b"mdash", '—'),
    (b"ndash", '–'),
    (b"lsquo", '‘'),
    (b"rsquo", '’'),
    (b"sbquo", '‚'),
    (b"ldquo", '“'),
    (b"rdquo", '”'),
    (b"bdquo", '„'),
    (b"laquo", '«'),
    (b"raquo", '»'),
    (b"bull", '•'),
    (b"middot", '·'),
    (b"deg", '°'),
    (b"euro", '€'),
    (b"pound", '£'),
    (b"yen", '¥'),
    (b"cent", '¢'),
    (b"sect", '§'),
    (b"para", '¶'),
    (b"times", '×'),
    (b"divide", '÷'),
    (b"plusmn", '±'),
    (b"frac12", '½'),
    (b"iexcl", '¡'),
    (b"iquest", '¿'),
    (b"aacute", 'á'),
    (b"agrave", 'à'),
    (b"acirc", 'â'),
    (b"auml", 'ä'),
    (b"aring", 'å'),
    (b"ccedil", 'ç'),
    (b"eacute", 'é'),
    (b"egrave", 'è'),
    (b"ecirc", 'ê'),
    (b"euml", 'ë'),
    (b"iacute", 'í'),
    (b"icirc", 'î'),
    (b"iuml", 'ï'),
    (b"ntilde", 'ñ'),
    (b"oacute", 'ó'),
    (b"ocirc", 'ô'),
    (b"ouml", 'ö'),
    (b"oslash", 'ø'),
    (b"uacute", 'ú'),
    (b"ucirc", 'û'),
    (b"uuml", 'ü'),
    (b"szlig", 'ß'),
    (b"Aacute", 'Á'),
    (b"Agrave", 'À'),
    (b"Auml", 'Ä'),
    (b"Ccedil", 'Ç'),
    (b"Eacute", 'É'),
    (b"Egrave", 'È'),
    (b"Ntilde", 'Ñ'),
    (b"Ouml", 'Ö'),
    (b"Uuml", 'Ü'),
];

/// What one scan wrote: the text, its links, and, for a styled scan, where each run starts and
/// where each image stood in the text.
struct Scanned {
    out: Vec<u8>,
    links: Vec<String>,
    starts: Vec<(usize, Style)>,
    images: Vec<(usize, Image)>,
}

/// `html` as plain text with link markers, and the addresses they stand for.
pub(crate) fn html_render(html: &str) -> Rendered {
    let Scanned { out, links, .. } = scan(html, false);
    // Only whole UTF-8 sequences of the input and whole encoded characters are written.
    let text = String::from_utf8(out)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    Rendered { text, links }
}

/// One scan of `html`; runs and images only when `styled`.
fn scan(html: &str, styled: bool) -> Scanned {
    let mut scanner = Scanner {
        bytes: html.as_bytes(),
        out: Vec::with_capacity(html.len().min(1 << 20)),
        space: false,
        newlines: 0,
        links: Numbering::default(),
        open: None,
        counts: Counts::default(),
        starts: styled.then(Vec::new),
        split: false,
        images: Vec::new(),
    };
    scanner.run();
    scanner.close_link();
    while scanner.out.last() == Some(&b'\n') {
        scanner.out.pop();
    }
    let len = scanner.out.len();
    let mut starts = scanner.starts.unwrap_or_default();
    starts.retain(|&(start, _)| start < len);
    Scanned {
        out: scanner.out,
        links: scanner.links.into_links(),
        starts,
        images: scanner.images,
    }
}

/// `html` as plain text (see the module documentation), for the scanner's own tests.
#[cfg(test)]
pub(crate) fn html_text(html: &str) -> String {
    html_render(html).text
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// A byte that ends a tag name.
fn ends_name(b: u8) -> bool {
    is_space(b) || b == b'/' || b == b'>'
}

/// The first `NAME_BYTES` bytes of a tag name, lower-cased; `None` when it is longer.
struct Name {
    bytes: [u8; NAME_BYTES],
    len: usize,
    long: bool,
}

impl Name {
    fn get(&self) -> Option<&[u8]> {
        (!self.long).then(|| &self.bytes[..self.len])
    }

    fn is_in(&self, list: &[&[u8]]) -> bool {
        self.get().is_some_and(|name| list.contains(&name))
    }
}

/// Open tags of each style, saturating: stray end tags stop at zero.
#[derive(Default)]
struct Counts {
    strong: u8,
    emphasis: u8,
    quote: u8,
}

impl Counts {
    /// Count the start or end tag `name`.
    fn tag(&mut self, name: &Name, closing: bool) {
        let count = if name.is_in(STRONG) {
            &mut self.strong
        } else if name.is_in(EMPHASIS) {
            &mut self.emphasis
        } else if name.is_in(QUOTE) {
            &mut self.quote
        } else {
            return;
        };
        *count = if closing {
            count.saturating_sub(1)
        } else {
            count.saturating_add(1)
        };
    }
}

struct Scanner<'a> {
    bytes: &'a [u8],
    out: Vec<u8>,
    /// White space seen since the last text.
    space: bool,
    /// Line breaks at the end of `out`.
    newlines: usize,
    /// The message's links so far.
    links: Numbering,
    /// The number of the `<a>` still open, whose marker goes after its text.
    open: Option<usize>,
    counts: Counts,
    /// Where each run of one style starts in `out`; `None` when only the text is wanted.
    starts: Option<Vec<(usize, Style)>>,
    /// An image was met since the last run started: the next run starts afresh.
    split: bool,
    /// The images met (styled scans only), each with where it stood in `out`.
    images: Vec<(usize, Image)>,
}

impl Scanner<'_> {
    fn run(&mut self) {
        let mut at = 0;
        while at < self.bytes.len() {
            at = match self.bytes[at] {
                b'<' => self.markup(at),
                b'&' => self.reference(at),
                b if is_space(b) => {
                    self.space = true;
                    at + 1
                }
                _ => self.text(at),
            };
        }
    }

    /// A run of text bytes from `at`.
    fn text(&mut self, at: usize) -> usize {
        let end = self.bytes[at..]
            .iter()
            .position(|&b| b == b'<' || b == b'&' || is_space(b))
            .map_or(self.bytes.len(), |p| at + p);
        self.visible(at, end);
        end
    }

    fn visible(&mut self, start: usize, end: usize) {
        self.begin(self.style());
        self.out.extend_from_slice(&self.bytes[start..end]);
        self.newlines = 0;
    }

    /// The style of text written now.
    fn style(&self) -> Style {
        Style {
            strong: self.counts.strong > 0,
            emphasis: self.counts.emphasis > 0,
            link: self.open.is_some(),
            quote: self.counts.quote > 0,
        }
    }

    /// Start a run at the end of `out` when `style` differs from the last one.
    fn mark(&mut self, style: Style) {
        if let Some(starts) = &mut self.starts
            && (self.split || starts.last().is_none_or(|&(_, last)| last != style))
        {
            starts.push((self.out.len(), style));
            self.split = false;
        }
    }

    /// An `<img>` whose attributes are `bytes[from..end]`, in a styled scan, up to
    /// `MAX_IMAGES`.
    fn image(&mut self, from: usize, end: usize) {
        if self.starts.is_none() || self.images.len() >= MAX_IMAGES {
            return;
        }
        if let Some(image) = image::read(&self.bytes[from.min(end)..end]) {
            self.images.push((self.out.len(), image));
            self.split = true;
        }
    }

    /// Before text in `style`: the pending space, in the styles shared by the text on both
    /// sides of it, then a run for `style`.
    fn begin(&mut self, style: Style) {
        if self.space && !self.out.is_empty() && self.newlines == 0 {
            let before = self
                .starts
                .as_ref()
                .and_then(|s| s.last())
                .map_or(Style::PLAIN, |&(_, last)| last);
            self.mark(Style {
                strong: before.strong && style.strong,
                emphasis: before.emphasis && style.emphasis,
                link: before.link && style.link,
                quote: before.quote && style.quote,
            });
            self.out.push(b' ');
        }
        self.space = false;
        self.mark(style);
    }

    fn push_char(&mut self, c: char) {
        if c == '\n' || c == '\t' {
            self.space = true;
            return;
        }
        self.begin(self.style());
        let mut buf = [0u8; 4];
        self.out
            .extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        self.newlines = 0;
    }

    fn newline(&mut self) {
        self.space = false;
        if !self.out.is_empty() && self.newlines < MAX_NEWLINES {
            self.mark(Style::PLAIN);
            self.out.push(b'\n');
            self.newlines += 1;
        }
    }

    /// An `<a>` whose attributes are `bytes[from..end]`: the marker of the one still open goes
    /// out first, then this one's address is numbered.
    fn open_link(&mut self, from: usize, end: usize) {
        self.close_link();
        let attributes = &self.bytes[from.min(end)..end];
        self.open = attribute(attributes, b"href")
            .and_then(decode_value)
            .and_then(|address| self.links.number(&address));
    }

    /// The marker of the open `<a>`, if it has one.
    fn close_link(&mut self) {
        if let Some(number) = self.open.take() {
            self.begin(Style {
                link: true,
                ..self.style()
            });
            self.out.extend_from_slice(format!("[{number}]").as_bytes());
            self.newlines = 0;
        }
    }

    /// What starts with `<` at `at`: a comment, a declaration, a tag, or a plain `<`.
    fn markup(&mut self, at: usize) -> usize {
        let rest = &self.bytes[at..];
        if rest.starts_with(b"<!--") {
            return find(self.bytes, at + 2, b"-->").map_or(self.bytes.len(), |e| e + 3);
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            return self.tag_end(at + 2, at);
        }
        let closing = rest.get(1) == Some(&b'/');
        let name_at = at + 1 + usize::from(closing);
        if !self.bytes.get(name_at).is_some_and(u8::is_ascii_alphabetic) {
            self.visible(at, at + 1);
            return at + 1;
        }
        let (name, after_name) = self.name(name_at, at);
        let end = self.tag_end(after_name, at);
        if !closing && name.get() == Some(b"img".as_slice()) {
            self.image(after_name, end);
            return end;
        }
        if name.get() == Some(b"a".as_slice()) {
            if closing {
                self.close_link();
            } else {
                self.open_link(after_name, end);
            }
            return end;
        }
        self.counts.tag(&name, closing);
        if name.is_in(BLOCK) {
            self.newline();
        } else if name.is_in(CELL) {
            self.space = true;
        }
        match name.get() {
            Some(b"head") if !closing => self.skip_head(end),
            Some(hidden) if !closing && HIDDEN.contains(&hidden) => self.skip_hidden(end, hidden),
            _ => end,
        }
    }

    /// The tag name at `at`, and where it ends (within the tag's byte cap from `start`).
    fn name(&self, at: usize, start: usize) -> (Name, usize) {
        let mut name = Name {
            bytes: [0; NAME_BYTES],
            len: 0,
            long: false,
        };
        let limit = self.bytes.len().min(start + MAX_TAG_BYTES);
        let mut end = at;
        while end < limit && !ends_name(self.bytes[end]) {
            if name.len < NAME_BYTES {
                name.bytes[name.len] = self.bytes[end].to_ascii_lowercase();
                name.len += 1;
            } else {
                name.long = true;
            }
            end += 1;
        }
        (name, end)
    }

    /// Just past the `>` that ends the tag begun at `start`, reading from `at`. A quote opens
    /// a value only after `=`; at `MAX_TAG_BYTES` (or the end of the text) the tag ends.
    fn tag_end(&self, at: usize, start: usize) -> usize {
        let limit = self.bytes.len().min(start + MAX_TAG_BYTES);
        let mut quote: Option<u8> = None;
        let mut after_equals = false;
        let from = at.min(limit);
        for (offset, &b) in self.bytes[from..limit].iter().enumerate() {
            let index = from + offset;
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'>' => return index + 1,
                None if b == b'=' => after_equals = true,
                None if (b == b'"' || b == b'\'') && after_equals => quote = Some(b),
                None if is_space(b) => {}
                None => after_equals = false,
            }
        }
        limit
    }

    /// Just past the end tag of hidden element `name` whose content starts at `at`.
    fn skip_hidden(&self, at: usize, name: &[u8]) -> usize {
        let mut at = at;
        while let Some(offset) = self.bytes[at..].iter().position(|&b| b == b'<') {
            let open = at + offset;
            if self.is_end_tag(open, name) {
                return self.tag_end(open + 2 + name.len(), open);
            }
            at = open + 1;
        }
        self.bytes.len()
    }

    /// Where normal text resumes after a `head` whose content starts at `at`. As in a
    /// browser, `head` ends at its end tag, at the first text that is not white space, and at
    /// any start tag that cannot stand in it (`body`, `p`, ...); the elements that can are
    /// skipped whole. Every step moves forward, so this stays linear.
    fn skip_head(&self, mut at: usize) -> usize {
        while at < self.bytes.len() {
            let b = self.bytes[at];
            if is_space(b) {
                at += 1;
                continue;
            }
            if b != b'<' {
                return at;
            }
            match self.head_markup(at) {
                HeadStep::Inside(next) => at = next,
                HeadStep::Leave(resume) => return resume,
            }
        }
        at
    }

    /// One piece of markup at `open` inside `head`.
    fn head_markup(&self, open: usize) -> HeadStep {
        let rest = &self.bytes[open..];
        if rest.starts_with(b"<!--") {
            return HeadStep::Inside(
                find(self.bytes, open + 2, b"-->").map_or(self.bytes.len(), |e| e + 3),
            );
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            return HeadStep::Inside(self.tag_end(open + 2, open));
        }
        if self.is_end_tag(open, b"head") {
            return HeadStep::Leave(self.tag_end(open + 2 + b"head".len(), open));
        }
        if rest.get(1) == Some(&b'/') {
            return HeadStep::Inside(self.tag_end(open + 2, open));
        }
        if !rest.get(1).is_some_and(u8::is_ascii_alphabetic) {
            return HeadStep::Leave(open);
        }
        let (name, after_name) = self.name(open + 1, open);
        let end = self.tag_end(after_name, open);
        match name.get() {
            Some(inner) if HIDDEN_IN_HEAD.contains(&inner) => {
                HeadStep::Inside(self.skip_hidden(end, inner))
            }
            Some(other) if IN_HEAD.contains(&other) => HeadStep::Inside(end),
            _ => HeadStep::Leave(open),
        }
    }

    /// Whether `</name` followed by the end of a name starts at `at`.
    fn is_end_tag(&self, at: usize, name: &[u8]) -> bool {
        self.bytes.get(at + 1) == Some(&b'/') && self.starts_tag(at + 2, name)
    }

    /// Whether `name` (case-insensitively) followed by the end of a name starts at `at`.
    fn starts_tag(&self, at: usize, name: &[u8]) -> bool {
        let Some(found) = self.bytes.get(at..at + name.len()) else {
            return false;
        };
        found.eq_ignore_ascii_case(name)
            && self
                .bytes
                .get(at + name.len())
                .is_none_or(|&b| ends_name(b))
    }

    /// A character reference at `at`, decoded; a plain `&` when it is not one Rail decodes.
    fn reference(&mut self, at: usize) -> usize {
        match decode_reference(self.bytes, at) {
            Some((c, end)) => {
                self.push_char(c);
                end
            }
            None => {
                self.visible(at, at + 1);
                at + 1
            }
        }
    }
}

/// The value of attribute `wanted` (lower case) among `attributes` (from the end of the tag
/// name to the end of the tag), as written. Quotes of other attributes are honoured, so
/// `title="href=x"` is not an `href`.
fn attribute<'a>(attributes: &'a [u8], wanted: &[u8]) -> Option<&'a [u8]> {
    let mut at = 0;
    while at < attributes.len() {
        while attributes
            .get(at)
            .is_some_and(|&b| is_space(b) || b == b'/')
        {
            at += 1;
        }
        let start = at;
        while attributes
            .get(at)
            .is_some_and(|&b| !is_space(b) && !matches!(b, b'=' | b'>' | b'/'))
        {
            at += 1;
        }
        let name = &attributes[start..at];
        while attributes.get(at).is_some_and(|&b| is_space(b)) {
            at += 1;
        }
        if attributes.get(at) != Some(&b'=') {
            at += usize::from(name.is_empty());
            continue;
        }
        at += 1;
        while attributes.get(at).is_some_and(|&b| is_space(b)) {
            at += 1;
        }
        let (value, next) = attribute_value(attributes, at);
        if name.eq_ignore_ascii_case(wanted) {
            return Some(value);
        }
        at = next;
    }
    None
}

/// The value starting at `at` (quoted or bare) and where scanning goes on.
fn attribute_value(attributes: &[u8], at: usize) -> (&[u8], usize) {
    match attributes.get(at) {
        Some(&quote @ (b'"' | b'\'')) => {
            let start = at + 1;
            let end = attributes[start..]
                .iter()
                .position(|&b| b == quote)
                .map_or(attributes.len(), |p| start + p);
            (&attributes[start..end], end + 1)
        }
        _ => {
            let start = at.min(attributes.len());
            let end = attributes[start..]
                .iter()
                .position(|&b| is_space(b) || b == b'>')
                .map_or(attributes.len(), |p| start + p);
            (&attributes[start..end], end)
        }
    }
}

/// An attribute value with its character references decoded and white space trimmed. A value
/// over twice `MAX_LINK_BYTES` is no address Rail keeps, so it is not decoded at all.
fn decode_value(value: &[u8]) -> Option<String> {
    if value.len() > 2 * MAX_LINK_BYTES {
        return None;
    }
    let mut out = Vec::with_capacity(value.len());
    let mut at = 0;
    while at < value.len() {
        if value[at] == b'&'
            && let Some((c, end)) = decode_reference(value, at)
        {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            at = end;
            continue;
        }
        out.push(value[at]);
        at += 1;
    }
    Some(String::from_utf8_lossy(&out).trim().to_string())
}

/// The character and the end of the reference starting with `&` at `at`.
fn decode_reference(bytes: &[u8], at: usize) -> Option<(char, usize)> {
    if bytes.get(at + 1) == Some(&b'#') {
        return decode_numeric(bytes, at + 2);
    }
    let start = at + 1;
    let len = bytes[start..]
        .iter()
        .take(MAX_ENTITY_NAME + 1)
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    if len == 0 || len > MAX_ENTITY_NAME || bytes.get(start + len) != Some(&b';') {
        return None;
    }
    let name = &bytes[start..start + len];
    let (_, c) = ENTITIES.iter().find(|(n, _)| *n == name)?;
    Some((*c, start + len + 1))
}

/// `&#N;` or `&#xH;` (the `;` optional), starting just past `#`.
fn decode_numeric(bytes: &[u8], at: usize) -> Option<(char, usize)> {
    let hex = matches!(bytes.get(at), Some(b'x' | b'X'));
    let start = at + usize::from(hex);
    let radix = if hex { 16 } else { 10 };
    let len = bytes[start.min(bytes.len())..]
        .iter()
        .take(MAX_DIGITS + 1)
        .take_while(|b| char::from(**b).is_digit(radix))
        .count();
    if len == 0 || len > MAX_DIGITS {
        return None;
    }
    let digits = std::str::from_utf8(&bytes[start..start + len]).ok()?;
    let code = u32::from_str_radix(digits, radix).ok()?;
    let allowed = code == 0x09 || code == 0x0a || (code >= 0x20 && !(0x7f..=0x9f).contains(&code));
    let c = char::from_u32(code).filter(|_| allowed)?;
    let end = start + len;
    Some((c, end + usize::from(bytes.get(end) == Some(&b';'))))
}

/// The first `needle` at or after `from`.
fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}
