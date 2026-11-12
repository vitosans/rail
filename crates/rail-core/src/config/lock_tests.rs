use super::lock::{lock_config, lock_path};

fn try_lock(path: &std::path::Path) -> bool {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path(path))
        .unwrap();
    match fs4::FileExt::try_lock(&file) {
        Ok(()) => true,
        Err(fs4::TryLockError::WouldBlock) => false,
        Err(fs4::TryLockError::Error(e)) => panic!("{e}"),
    }
}

#[test]
fn a_held_lock_shuts_out_every_other_holder_until_it_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let held = lock_config(&config).unwrap();
    assert!(!try_lock(&config), "a second open file cannot take it");
    drop(held);
    let mut freed = false;
    for _ in 0..50 {
        if try_lock(&config) {
            freed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(freed, "free once dropped");
}

#[test]
fn the_lock_sits_beside_the_config_as_named() {
    let config = std::path::Path::new("/somewhere/rail/config.toml");
    assert_eq!(
        lock_path(config),
        std::path::Path::new("/somewhere/rail/config.lock")
    );
}

#[cfg(unix)]
#[test]
fn a_missing_directory_is_made_private_and_an_existing_one_is_left_alone() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();

    let fresh = dir.path().join("fresh");
    drop(lock_config(&fresh.join("config.toml")).unwrap());
    assert_eq!(mode(&fresh), 0o700);
    assert_eq!(mode(&fresh.join("config.lock")), 0o600);

    let open = dir.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    drop(lock_config(&open.join("config.toml")).unwrap());
    assert_eq!(mode(&open), 0o755);
}
