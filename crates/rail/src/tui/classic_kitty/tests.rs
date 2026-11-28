use super::*;

fn bitmap(width: u16, height: u16) -> Arc<Bitmap> {
    let size = Size::new(width, height);
    let image = DynamicImage::new_rgba8(u32::from(width) * 2, u32::from(height) * 2);
    Bitmap::encode(&image, size, (2, 2)).unwrap()
}

fn frame(layer: &mut Layer, placements: &[Placement]) -> String {
    let mut output = Vec::new();
    layer.draw(&mut output, placements).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn clips_the_source_pixels_to_the_visible_cells() {
    let image = bitmap(6, 4);
    let area = Rect::new(9, 12, 3, 2);
    assert!(Placement::clipped(image.clone(), area, -4).is_none());
    assert!(Placement::clipped(image.clone(), area, 2).is_none());
    let clipped = Placement::clipped(image, area, -1).unwrap();
    let commands = frame(&mut Layer::default(), &[clipped]);
    assert!(commands.contains("\x1b[13;10H"), "{commands}");
    assert!(commands.contains("x=0,y=2,w=6,h=4,c=3,r=2,C=1"));
    assert!(commands.starts_with("\x1b7") && commands.ends_with("\x1b8"));
}

#[test]
fn shares_transfers_updates_placements_and_frees_only_owned_images() {
    let image = bitmap(2, 3);
    let area = Rect::new(1, 2, 9, 20);
    let first = Placement::clipped(image.clone(), area, 0).unwrap();
    let second = Placement::clipped(image.clone(), area, 10).unwrap();
    let mut layer = Layer::default();
    let both = [first, second.clone()];
    let initial = frame(&mut layer, &both);
    assert_eq!(initial.matches("a=t,").count(), 1);
    assert_eq!(initial.matches("a=p,").count(), 2);
    assert!(!initial.contains("U=1"));
    assert!(frame(&mut layer, &both).is_empty());
    let remaining = frame(&mut layer, &[second]);
    assert!(remaining.contains(&format!("a=d,d=i,i={},p=", image.id)));
    assert!(!remaining.contains("a=t,"));
    let closed = frame(&mut layer, &[]);
    assert!(closed.contains(&format!("a=d,d=I,i={},", image.id)));
    assert!(!closed.contains("d=A"));
    assert!(frame(&mut layer, &[]).is_empty());
}

#[test]
fn raw_rgba_survives_base64_chunk_boundaries_and_keeps_transparent_padding() {
    let source = image::RgbaImage::from_pixel(64, 32, image::Rgba([1, 2, 3, 255]));
    let bitmap = Bitmap::encode(&source.into(), Size::new(32, 32), (2, 2)).unwrap();
    let placement = Placement::clipped(bitmap, Rect::new(0, 0, 32, 32), 0).unwrap();
    let wire = frame(&mut Layer::default(), &[placement]);
    let mut data = String::new();
    let expected_len = base64::encoded_len(64 * 64 * 4, true).unwrap();
    for part in wire.split("\x1b_G").skip(1) {
        let command = part.split("\x1b\\").next().unwrap();
        if let Some((header, chunk)) = command.split_once(';') {
            assert!(chunk.len() <= 4096);
            assert_eq!(
                header.ends_with("m=0"),
                chunk.len() + data.len() == expected_len
            );
            data.push_str(chunk);
        }
    }
    let raw = STANDARD.decode(data).unwrap();
    assert_eq!(raw.len(), 64 * 64 * 4);
    assert_eq!(&raw[..4], &[1, 2, 3, 255]);
    assert_eq!(&raw[64 * 32 * 4..][..4], &[0; 4]);
}

#[test]
fn terminal_geometry_bounds_the_raster_even_for_extreme_cell_sizes() {
    assert!(canvas(Size::new(1, 0), (8, 16)).is_none());
    assert!(canvas(Size::new(1, 2), (0, 16)).is_none());
    for size in [Size::new(u16::MAX, u16::MAX), Size::new(u16::MAX, 1)] {
        let (width, height) = canvas(size, (u16::MAX, 1)).unwrap();
        assert!(width.max(height) <= 8192);
        assert!(u64::from(width) * u64::from(height) <= 1_048_576);
    }
}

#[test]
fn a_partial_placement_is_terminated_before_the_cursor_is_restored_or_cleanup_retried() {
    struct InterruptedPacket {
        bytes: Vec<u8>,
        limit: usize,
        failed: bool,
    }
    impl Write for InterruptedPacket {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len() == self.limit && !self.failed {
                self.failed = true;
                return Err(io::Error::other("interrupted placement"));
            }
            let count = if self.failed {
                bytes.len()
            } else {
                bytes.len().min(self.limit - self.bytes.len())
            };
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let image = bitmap(3, 3);
    let placement = Placement::clipped(image, Rect::new(0, 0, 9, 8), 0).unwrap();
    let placements = [placement];
    let wire = frame(&mut Layer::default(), &placements);
    let limit = wire.find("a=p,").unwrap() + 4;
    let mut writer = InterruptedPacket {
        bytes: Vec::new(),
        limit,
        failed: false,
    };
    let mut layer = Layer::default();
    assert!(layer.draw(&mut writer, &placements).is_err());
    assert!(writer.bytes.ends_with(b"a=p,\x1b\\\x1b8"));
    let cleanup = frame(&mut layer, &[]);
    assert!(cleanup.starts_with("\x1b\\\x1b7\x1b_Ga=d,d=I"));
}

#[test]
fn a_failed_flush_cleans_up_an_uncertain_transfer_even_if_the_reader_closes() {
    struct FailFlush;
    impl Write for FailFlush {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush"))
        }
    }
    let mut layer = Layer::default();
    let image = bitmap(3, 3);
    let placement = Placement::clipped(image.clone(), Rect::new(0, 0, 9, 8), 0).unwrap();
    assert!(layer.draw(&mut FailFlush, &[placement]).is_err());
    assert!(frame(&mut layer, &[]).contains(&format!("d=I,i={}", image.id)));
    assert!(frame(&mut layer, &[]).is_empty());
}
