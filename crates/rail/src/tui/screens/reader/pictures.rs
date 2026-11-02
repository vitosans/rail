//! Pictures in rich mode: the images of an HTML body, each fetched by the daemon (a `cid:` one
//! with `attachment.image`, an `https:` one with `image.remote`), decoded here off the UI
//! thread under memory limits, and sized as a browser would draw it: the size the mail
//! declares (or the picture's own), at `CSS_PX_PER_COL` CSS pixels to a column, rows from the
//! terminal cell's shape, shrunk to the pane and `MAX_PICTURE_ROWS`. A picture of
//! `MAX_HIDDEN_PX` or less on a side is a tracking pixel or a spacer and takes no room.
//! Fetching a remote picture tells its sender the mail was read, so a message's remote
//! pictures are asked for only once they are loaded: `remote_images`, the key for this thread,
//! or a sender in `remote_images_from` whose domain the receiving server vouched for (DMARC or
//! aligned DKIM, `auth_results`), since a From line alone can be forged.
//! A reader asks for at most `MAX_PICTURES_PER_READER`, `MAX_PICTURES_IN_FLIGHT` at a time,
//! and keeps each shrunk to `MAX_KEPT_PIXELS`, so one mail cannot fill memory.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use image::{DynamicImage, ImageReader, Limits};
use rail_core::provider::auth_results::aligned;
use rail_core::provider::html::Image;
use rail_core::store::view::{BodyState, MessageView};
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::SlicedProtocol;
use ratatui_image::{FilterType, Resize};

use super::Reader;
use super::rich::RichBody;

/// CSS pixels to one terminal column, as a mail client's reading pane draws them.
const CSS_PX_PER_COL: f64 = 8.0;
/// The most rows one picture takes.
pub const MAX_PICTURE_ROWS: u16 = 30;
/// A side this small or smaller hides the picture.
const MAX_HIDDEN_PX: u32 = 4;
/// The largest side Rail decodes, in pixels.
const MAX_DECODED_SIDE: u32 = 8192;
/// The most memory one decode may take.
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;
/// The most pixels a decoded picture keeps (4 MiB as RGBA): `MAX_PICTURE_ROWS` of even
/// 32-pixel cells is under a thousand pixels, so more than the reader ever draws.
pub const MAX_KEPT_PIXELS: u64 = 1024 * 1024;
/// The most pictures one open reader asks for; the rest stay their placeholders.
pub const MAX_PICTURES_PER_READER: usize = 32;
/// The most pictures one reader has being fetched and decoded at once.
pub const MAX_PICTURES_IN_FLIGHT: usize = 4;
/// The longest Content-ID, as the store keeps it.
const MAX_CONTENT_ID_BYTES: usize = 255;
/// The longest remote address the daemon fetches.
const MAX_REMOTE_URL_BYTES: usize = rail_core::ops::MAX_REMOTE_URL_BYTES;

/// Where a picture comes from: the message's own part with this Content-ID, or this `https:`
/// address.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Inline(String),
    Remote(String),
}

/// The kind only: a Content-ID or an address is the sender's text.
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Inline(_) => "Inline",
            Self::Remote(_) => "Remote",
        })
    }
}

/// One picture of a body: before run `at`, from `source`.
#[derive(Clone, PartialEq, Eq)]
pub struct Inline {
    pub at: usize,
    pub source: Source,
    /// Outside text, as the scanner cleaned it.
    pub alt: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A picture decoded and shrunk to `MAX_KEPT_PIXELS`; `natural` is its own size, which the
/// sizing goes by.
pub struct DecodedPicture {
    pub image: DynamicImage,
    pub natural: (u32, u32),
}

/// Where a picture is: asked for, drawn, or not to be had (the placeholder stays).
#[derive(Clone)]
pub enum Picture {
    Loading,
    Ready(Rc<DecodedPicture>),
    Failed,
}

/// The `cid:` and `https:` pictures among `images`, tiny declared ones left out.
pub fn inline(images: Vec<Image>) -> Vec<Inline> {
    images
        .into_iter()
        .filter(|image| {
            let tiny = |side: Option<u32>| side.is_some_and(|px| px <= MAX_HIDDEN_PX);
            !tiny(image.width) && !tiny(image.height)
        })
        .filter_map(|image| {
            let source = match content_id(&image.src) {
                Some(id) => Source::Inline(id),
                None => Source::Remote(remote(&image.src)?),
            };
            Some(Inline {
                source,
                at: image.at,
                alt: image.alt,
                width: image.width,
                height: image.height,
            })
        })
        .collect()
}

/// A `cid:` address's Content-ID (RFC 2392: the rest of the address, percent-decoded); `None`
/// for any other address, or one the store would not have kept.
pub fn content_id(src: &str) -> Option<String> {
    let rest = src
        .get(..4)
        .filter(|scheme| scheme.eq_ignore_ascii_case("cid:"))
        .map(|_| &src[4..])?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    let kept = !out.is_empty()
        && out.len() <= MAX_CONTENT_ID_BYTES
        && out.iter().all(u8::is_ascii_graphic);
    kept.then(|| String::from_utf8_lossy(&out).into_owned())
}

/// An `https:` address, trimmed, short enough for the daemon to take; `None` for any other.
/// Plain `http:` is never fetched, so it stays its placeholder.
fn remote(src: &str) -> Option<String> {
    let src = src.trim();
    let https = src
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    (https && src.len() > 8 && src.len() <= MAX_REMOTE_URL_BYTES).then(|| src.to_string())
}

/// `m`'s first sender's address, in lower case.
fn sender(m: &MessageView) -> Option<String> {
    m.from.first().map(|p| p.address.to_lowercase())
}

/// `m`'s sender is one whose remote pictures are always fetched, and the receiving server
/// vouched for the sender's domain: a From line alone can be forged.
pub(super) fn trusts(trusted: &HashSet<String>, m: &MessageView) -> bool {
    sender(m)
        .is_some_and(|address| trusted.contains(&address) && aligned(&address, &m.vouched_domains))
}

/// The picture at `path` decoded, within `MAX_DECODED_SIDE` and `MAX_DECODE_ALLOC`, and
/// shrunk to `MAX_KEPT_PIXELS`. An animated picture gives its first frame. The error is the
/// decoder's, never the path.
pub fn decode(path: &Path) -> Result<DecodedPicture, String> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let mut reader = ImageReader::open(path)
        .and_then(ImageReader::with_guessed_format)
        .map_err(|error| error.kind().to_string())?;
    reader.limits(limits);
    let image = reader.decode().map_err(|error| error.to_string())?;
    let natural = (image.width(), image.height());
    Ok(DecodedPicture {
        image: shrunk(image),
        natural,
    })
}

/// `image` within `MAX_KEPT_PIXELS`, its shape kept.
fn shrunk(image: DynamicImage) -> DynamicImage {
    let pixels = u64::from(image.width()) * u64::from(image.height());
    if pixels <= MAX_KEPT_PIXELS {
        return image;
    }
    let scale = (MAX_KEPT_PIXELS as f64 / pixels as f64).sqrt();
    let side = |px: u32| ((f64::from(px) * scale).floor() as u32).max(1);
    image.resize(
        side(image.width()),
        side(image.height()),
        FilterType::Triangle,
    )
}

/// The cells a picture of `natural` pixels takes at `width` columns, with cells of `font`
/// pixels (width, height): `None` when it is too small to show.
pub fn fit(
    natural: (u32, u32),
    declared: (Option<u32>, Option<u32>),
    width: u16,
    font: (u16, u16),
) -> Option<(u16, u16)> {
    if natural.0 <= MAX_HIDDEN_PX || natural.1 <= MAX_HIDDEN_PX {
        return None;
    }
    let (nw, nh) = (f64::from(natural.0), f64::from(natural.1));
    let (w, h) = match declared {
        (Some(w), Some(h)) => (f64::from(w), f64::from(h)),
        (Some(w), None) => (f64::from(w), f64::from(w) * nh / nw),
        (None, Some(h)) => (f64::from(h) * nw / nh, f64::from(h)),
        (None, None) => (nw, nh),
    };
    let shape = f64::from(font.0.max(1)) / f64::from(font.1.max(1));
    let cols = (w / CSS_PX_PER_COL).max(1.0);
    let rows = (cols * (h / w) * shape).max(1.0);
    let scale = (f64::from(width.max(1)) / cols)
        .min(f64::from(MAX_PICTURE_ROWS) / rows)
        .min(1.0);
    let cells = |n: f64| (n * scale).round().clamp(1.0, f64::from(u16::MAX)) as u16;
    Some((cells(cols), cells(rows)))
}

/// The pictures asked for or read, by (message id, source).
pub type Pictures = HashMap<(i64, Source), Picture>;

/// Each picture encoded for the size it was last drawn at, by (message id, source): one entry
/// a picture, replaced when the size changes.
pub(super) type Protocols = HashMap<(i64, Source), ((u16, u16), Rc<SlicedProtocol>)>;

/// A picture to ask the daemon for: (message id, its provider id, source).
pub type Wanted = (i64, String, Source);

impl Reader {
    /// The reader with pictures drawn by `picker`, or none (`None`: images off).
    pub fn with_images(mut self, picker: Option<Picker>) -> Self {
        self.picker = picker;
        self
    }

    /// Pictures drawn by `picker` from now on, or none (`:images`).
    pub fn set_images(&mut self, picker: Option<Picker>) {
        self.picker = picker;
        self.protocols.borrow_mut().clear();
        self.changed();
    }

    /// Remote pictures fetched for this thread from the start (`remote_images`) or not, the
    /// senders whose are always fetched (lower case), and the hint while they are not.
    pub fn with_remote(mut self, loaded: bool, trusted: HashSet<String>, hint: String) -> Self {
        self.remote = loaded;
        self.trusted = trusted;
        self.remote_hint = hint;
        self
    }

    /// The senders whose remote pictures are always fetched, from now on.
    pub fn set_trusted(&mut self, trusted: HashSet<String>) {
        self.trusted = trusted;
        self.changed();
    }

    /// The cursor message's sender, in lower case.
    pub fn sender_at_cursor(&self) -> Option<String> {
        self.message_at_cursor().and_then(sender)
    }

    /// `:remote-images` flipped: from now on, this thread's remote pictures fetched or not.
    pub fn set_remote(&mut self, loaded: bool) {
        self.remote = loaded;
        self.changed();
    }

    /// The key: this thread's remote pictures fetched from now on. False when they already
    /// were, so nothing changed.
    pub fn load_remote(&mut self) -> bool {
        if self.remote {
            return false;
        }
        self.set_remote(true);
        true
    }

    /// What `lines::State::pictures` takes: in rich mode with images on, the pictures read so
    /// far and the cell's size.
    pub(super) fn placing(&self) -> Option<(&Pictures, (u16, u16))> {
        let picker = self.picker.as_ref().filter(|_| self.rich)?;
        let font = picker.font_size();
        Some((&self.pictures, (font.width, font.height)))
    }

    /// While this thread has not loaded its remote pictures: the hint, and the senders whose
    /// messages need none.
    pub(super) fn held(&self) -> Option<(&str, &HashSet<String>)> {
        (!self.remote).then_some((self.remote_hint.as_str(), &self.trusted))
    }

    /// Each open message whose body is read, with its HTML scanned.
    fn open_bodies(&self) -> impl Iterator<Item = (&MessageView, &RichBody)> {
        self.detail
            .messages
            .iter()
            .zip(&self.expanded)
            .filter(|(m, open)| **open && matches!(m.body, BodyState::Text { .. }))
            .filter_map(|(m, _)| Some((m, self.rich_bodies.get(&m.id)?.as_ref())))
    }

    /// The open messages' pictures not asked for yet: as many as `MAX_PICTURES_IN_FLIGHT` and
    /// `MAX_PICTURES_PER_READER` leave room for; none in plain mode or with images off, and no
    /// remote one until the thread's remote pictures are loaded.
    pub fn pictures_wanted(&self) -> Vec<Wanted> {
        if self.placing().is_none() {
            return Vec::new();
        }
        // A message gone from the thread drops its answers, so its pictures never stop loading.
        let in_thread = |id: &i64| self.detail.messages.iter().any(|m| m.id == *id);
        let loading = self
            .pictures
            .iter()
            .filter(|((id, _), p)| matches!(p, Picture::Loading) && in_thread(id))
            .count();
        let room = MAX_PICTURES_IN_FLIGHT
            .saturating_sub(loading)
            .min(MAX_PICTURES_PER_READER.saturating_sub(self.pictures.len()));
        let mut wanted = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (m, body) in self.open_bodies() {
            for inline in &body.pictures {
                if wanted.len() == room {
                    return wanted;
                }
                let held = !self.remote && !trusts(&self.trusted, m);
                if held && matches!(inline.source, Source::Remote(_)) {
                    continue;
                }
                let key = (m.id, inline.source.clone());
                if !self.pictures.contains_key(&key) && seen.insert(key) {
                    wanted.push((m.id, m.remote_id.clone(), inline.source.clone()));
                }
            }
        }
        wanted
    }

    /// `wanted` asked for: never asked again while the reader is open.
    pub fn mark_pictures_asked(&mut self, wanted: &[Wanted]) {
        for (id, _, source) in wanted {
            self.pictures
                .insert((*id, source.clone()), Picture::Loading);
        }
    }

    /// A picture read: drawn from now on, or (`None`) left as its placeholder.
    pub fn picture_read(&mut self, id: i64, source: Source, image: Option<DecodedPicture>) {
        let state = image.map_or(Picture::Failed, |image| Picture::Ready(Rc::new(image)));
        self.pictures.insert((id, source), state);
        self.changed();
    }

    /// Picture `picture` of message `message` encoded for `cols` by `rows` cells, again only
    /// when its size changes; `None` when it is not read, or images are off.
    pub fn protocol(
        &self,
        message: usize,
        picture: usize,
        cols: u16,
        rows: u16,
    ) -> Option<Rc<SlicedProtocol>> {
        let picker = self.picker.as_ref()?;
        let m = self.detail.messages.get(message)?;
        let inline = self.rich_bodies.get(&m.id)?.pictures.get(picture)?;
        let key = (m.id, inline.source.clone());
        if let Some((_, found)) = self
            .protocols
            .borrow()
            .get(&key)
            .filter(|(size, _)| *size == (cols, rows))
        {
            return Some(found.clone());
        }
        let Some(Picture::Ready(decoded)) = self.pictures.get(&key) else {
            return None;
        };
        // Scaled, not fit: a mail declares the size it wants drawn, often larger than the
        // picture itself, and a browser stretches it to that size.
        let encoded = SlicedProtocol::new_with_resize(
            picker,
            decoded.image.clone(),
            Size::new(cols, rows),
            Resize::Scale(Some(FilterType::Triangle)),
        );
        let encoded = Rc::new(encoded.ok()?);
        self.protocols
            .borrow_mut()
            .insert(key, ((cols, rows), encoded.clone()));
        Some(encoded)
    }
}
