//! The image cache's size cap, at the level of the trim that keeps it: the oldest pictures go
//! first until the new one fits, and a directory or symlink is never removed. A real cap is
//! hundreds of MiB, too much to write from a test, so the trim is driven with a small one.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::attachment_image::trim;

/// A cached picture of `size` bytes, last changed `seconds` ago.
fn picture(dir: &Path, name: &str, size: usize, seconds: u64) {
    let path = dir.join(name);
    fs::write(&path, vec![0u8; size]).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(seconds))
        .unwrap();
}

fn left(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_oldest_pictures_go_until_the_new_one_fits_under_the_cap() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    picture(dir, "oldest.png", 40, 300);
    picture(dir, "older.png", 40, 200);
    picture(dir, "newest.png", 40, 100);
    assert_eq!(trim(dir, 100, 30).unwrap(), 2);
    assert_eq!(left(dir), ["newest.png"]);
}

#[test]
fn a_cache_under_the_cap_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    picture(dir, "a.png", 40, 300);
    picture(dir, "b.png", 40, 200);
    assert_eq!(trim(dir, 100, 20).unwrap(), 0);
    assert_eq!(left(dir), ["a.png", "b.png"]);
}

#[test]
fn a_directory_or_a_symlink_is_neither_counted_nor_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    picture(dir, "a.png", 60, 300);
    fs::create_dir(dir.join("nested")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    picture(outside.path(), "big.png", 500, 400);
    symlink(outside.path().join("big.png"), dir.join("link.png")).unwrap();
    assert_eq!(trim(dir, 100, 30).unwrap(), 0);
    assert_eq!(left(dir), ["a.png", "link.png", "nested"]);
    assert!(outside.path().join("big.png").exists());
}

#[test]
fn a_missing_cache_has_nothing_to_trim() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(trim(&tmp.path().join("gone"), 100, 30).unwrap(), 0);
}

/// Another write's temporary file is in flight, not cached: removing it would fail that write.
#[test]
fn a_picture_still_being_written_is_left_for_its_writer() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    picture(dir, ".abc.part", 90, 300);
    picture(dir, "a.png", 40, 100);
    assert_eq!(trim(dir, 100, 70).unwrap(), 1);
    assert_eq!(left(dir), [".abc.part"]);
}
