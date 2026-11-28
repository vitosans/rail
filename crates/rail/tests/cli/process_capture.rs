use std::io::{ErrorKind, Read};
use std::os::fd::AsFd;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use super::KillsChild;

/// Capture both pipes while the child runs and kill it if the deadline expires.
pub(super) fn finish_within(child: Child, limit: Duration) -> Output {
    let mut running = KillsChild(Some(child));
    let child = running.0.as_mut().unwrap();
    drop(child.stdin.take());
    let mut stdout_pipe = nonblocking(child.stdout.take());
    let mut stderr_pipe = nonblocking(child.stderr.take());
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let deadline = Instant::now() + limit;
    loop {
        let status = child.try_wait().unwrap();
        let read_stdout = read_chunk(&mut stdout_pipe, &mut stdout);
        let read_stderr = read_chunk(&mut stderr_pipe, &mut stderr);
        if !read_stdout && !read_stderr {
            // A descendant may still hold a pipe open after the direct child exits.
            if let Some(status) = status {
                running.0.take();
                return Output {
                    status,
                    stdout,
                    stderr,
                };
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            Instant::now() < deadline,
            "child capture exceeded {limit:?}"
        );
    }
}

fn nonblocking<P: AsFd>(pipe: Option<P>) -> Option<P> {
    if let Some(pipe) = &pipe {
        rustix::io::ioctl_fionbio(pipe, true).expect("nonblocking child capture");
    }
    pipe
}

fn read_chunk(pipe: &mut Option<impl Read>, output: &mut Vec<u8>) -> bool {
    let Some(reader) = pipe else { return false };
    let mut chunk = [0; 8192];
    match reader.read(&mut chunk) {
        Ok(0) => {
            *pipe = None;
            false
        }
        Ok(count) => {
            output.extend_from_slice(&chunk[..count]);
            true
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => false,
        Err(error) if error.kind() == ErrorKind::Interrupted => true,
        Err(error) => panic!("child capture failed: {error}"),
    }
}

#[test]
fn captures_both_streams_larger_than_pipe_capacity() {
    let child = Command::new("sh")
        .args([
            "-c",
            r#"emit() { printf '%s' "$1"; dd if=/dev/zero bs=65536 count=32 2>/dev/null; }
               emit O & emit E >&2; wait; exit 17"#,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = finish_within(child, Duration::from_secs(30));
    assert_eq!(out.status.code(), Some(17));
    for (stream, marker) in [(out.stdout, b'O'), (out.stderr, b'E')] {
        assert_eq!(stream.len(), 1 + 65536 * 32);
        assert_eq!(stream[0], marker);
        assert!(stream[1..].iter().all(|&byte| byte == 0));
    }
}
