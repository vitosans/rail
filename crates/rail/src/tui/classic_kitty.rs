//! Normal Kitty image placements for multiplexers that cannot draw Unicode placeholders.
//! Upload pixels once while visible. Scroll and resize change placements independently.

use std::collections::HashSet;
use std::fmt;
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::{DynamicImage, RgbaImage, imageops};
use ratatui::layout::{Rect, Size};

pub(crate) fn in_zellij_kitty() -> bool {
    std::env::var_os("ZELLIJ").is_some()
        && std::env::var_os("KITTY_WINDOW_ID").is_some()
        && std::env::var_os("TMUX").is_none()
}

// A randomized starting point avoids colliding with earlier programs drawing in the same pane.
static NEXT_ID: LazyLock<AtomicU32> =
    LazyLock::new(|| AtomicU32::new(RandomState::new().hash_one("Rail Kitty images") as u32));

pub(crate) struct Bitmap {
    id: u32,
    cells: Size,
    width: u32,
    height: u32,
    data: String,
}

impl fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bitmap")
            .field("id", &self.id)
            .field("cells", &self.cells)
            .finish_non_exhaustive()
    }
}

impl Bitmap {
    pub(crate) fn encode(image: &DynamicImage, size: Size, font: (u16, u16)) -> Option<Arc<Self>> {
        if [image.width(), image.height()].contains(&0) {
            return None;
        }
        let (width, height) = canvas(size, font)?;
        let scaled = image
            .resize(width, height, imageops::FilterType::Triangle)
            .into_rgba8();
        let rgba = if scaled.dimensions() == (width, height) {
            scaled
        } else {
            let mut padded = RgbaImage::new(width, height);
            imageops::replace(&mut padded, &scaled, 0, 0);
            padded
        };
        let id = loop {
            let next = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            if next != 0 {
                break next;
            }
        };
        Some(Arc::new(Self {
            id,
            cells: size,
            width,
            height,
            data: STANDARD.encode(rgba.as_raw()),
        }))
    }

    fn transfer(&self, out: &mut impl Write) -> io::Result<()> {
        let mut chunks = self.data.as_bytes().chunks(4096).peekable();
        write!(
            out,
            "\x1b_Ga=t,t=d,f=32,i={},s={},v={},",
            self.id, self.width, self.height
        )?;
        while let Some(chunk) = chunks.next() {
            let more = u8::from(chunks.peek().is_some());
            write!(out, "q=2,m={more};")?;
            out.write_all(chunk)?;
            out.write_all(b"\x1b\\")?;
            if more != 0 {
                out.write_all(b"\x1b_G")?;
            }
        }
        Ok(())
    }
}

/// Bound the encoder like the reader's decoded cache, including transparent padding for
/// letterboxing. Large cell geometries still display in full using the terminal's scaler.
fn canvas(size: Size, font: (u16, u16)) -> Option<(u32, u32)> {
    let width = u32::from(size.width) * u32::from(font.0);
    let height = u32::from(size.height) * u32::from(font.1);
    if width == 0 || height == 0 {
        return None;
    }
    let scale = (1_048_576.0 / (f64::from(width) * f64::from(height)))
        .sqrt()
        .min(8192.0 / f64::from(width.max(height)))
        .min(1.0);
    let side = |pixels| ((f64::from(pixels) * scale) as u32).max(1);
    Some((side(width), side(height)))
}

#[derive(Clone, Debug)]
pub(crate) struct Placement {
    bitmap: Arc<Bitmap>,
    destination: Rect,
    source_top: u32,
    source_width: u32,
    source_height: u32,
}

impl PartialEq for Placement {
    fn eq(&self, other: &Self) -> bool {
        self.bitmap.id == other.bitmap.id
            && self.destination == other.destination
            && self.source_top == other.source_top
            && self.source_width == other.source_width
            && self.source_height == other.source_height
    }
}

impl Placement {
    pub(crate) fn clipped(bitmap: Arc<Bitmap>, area: Rect, top: i16) -> Option<Self> {
        let cells = bitmap.cells;
        let width = area.width.min(cells.width);
        let top = i32::from(top);
        let first = top.max(0);
        let end = (top + i32::from(cells.height)).min(i32::from(area.height));
        if width == 0 || end <= first {
            return None;
        }
        let height = (end - first) as u16;
        let pixel_row = |row: i32| {
            (u64::from(row as u32) * u64::from(bitmap.height) / u64::from(cells.height)) as u32
        };
        let source_top = pixel_row(first - top);
        let source_height = (pixel_row(end - top) - source_top).max(1);
        let source_width =
            (u64::from(width) * u64::from(bitmap.width) / u64::from(cells.width)) as u32;
        Some(Self {
            bitmap,
            destination: Rect::new(area.x, area.y.saturating_add(first as u16), width, height),
            source_top,
            source_width: source_width.max(1),
            source_height,
        })
    }

    fn placement_id(&self) -> u32 {
        // A visible cell has x,y < u16::MAX; one image can occur at several distinct cells.
        1 + ((u32::from(self.destination.y) << 16) | u32::from(self.destination.x))
    }

    fn place(&self, out: &mut impl Write) -> io::Result<()> {
        let rect = self.destination;
        write!(
            out,
            "\x1b[{};{}H\x1b_Ga=p,i={},p={},x=0,y={},w={},h={},c={},r={},C=1,q=2\x1b\\",
            u32::from(rect.y) + 1,
            u32::from(rect.x) + 1,
            self.bitmap.id,
            self.placement_id(),
            self.source_top,
            self.source_width,
            self.source_height,
            rect.width,
            rect.height
        )
    }
}

#[derive(Default)]
pub(crate) struct Layer {
    shown: Vec<Placement>,
    // A short write or failed flush leaves terminal state uncertain. Clear touched IDs on retry.
    uncertain: HashSet<u32>,
}

impl Layer {
    pub(crate) fn draw(
        &mut self,
        out: &mut impl Write,
        placements: &[Placement],
    ) -> io::Result<()> {
        if self.shown == placements && self.uncertain.is_empty() {
            return Ok(());
        }
        if !self.uncertain.is_empty() {
            // A previous write may have stopped inside an APC, including during recovery.
            out.write_all(b"\x1b\\")?;
        }
        out.write_all(b"\x1b7")?;
        let update = self.update(out, placements);
        let restore_cursor: &[u8] = if update.is_err() {
            b"\x1b\\\x1b8"
        } else {
            b"\x1b8"
        };
        let restore = out.write_all(restore_cursor).and_then(|()| out.flush());
        if let Err(error) = update.and(restore) {
            self.uncertain
                .extend(self.shown.iter().chain(placements).map(|p| p.bitmap.id));
            return Err(error);
        }
        self.shown.clear();
        self.shown.extend_from_slice(placements);
        self.uncertain.clear();
        Ok(())
    }

    fn update(&self, out: &mut impl Write, placements: &[Placement]) -> io::Result<()> {
        let visible: HashSet<_> = placements.iter().map(|p| p.bitmap.id).collect();
        let mut freed = self.uncertain.clone();
        for &id in &self.uncertain {
            delete_image(out, id)?;
        }
        for old in &self.shown {
            let id = old.bitmap.id;
            if freed.contains(&id) || placements.contains(old) {
                continue;
            }
            if visible.contains(&id) {
                write!(
                    out,
                    "\x1b_Ga=d,d=i,i={id},p={},q=2\x1b\\",
                    old.placement_id()
                )?;
            } else {
                delete_image(out, id)?;
                freed.insert(id);
            }
        }
        let mut uploaded: HashSet<_> = self.shown.iter().map(|p| p.bitmap.id).collect();
        uploaded.retain(|id| !freed.contains(id));
        for next in placements {
            let id = next.bitmap.id;
            if self.uncertain.contains(&id) || !self.shown.contains(next) {
                if uploaded.insert(id) {
                    next.bitmap.transfer(out)?;
                }
                next.place(out)?;
            }
        }
        Ok(())
    }
}

fn delete_image(out: &mut impl Write, id: u32) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

#[cfg(test)]
mod tests;
