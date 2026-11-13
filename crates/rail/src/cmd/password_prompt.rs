//! Prompt for a password without terminal echo, falling back to stdin when piped.

use std::io::{self, BufRead, Write};

/// Read a password without echo on a terminal, or read a line from stdin when piped.
pub fn read_password(prompt: &str) -> io::Result<String> {
    if !rustix::termios::isatty(rustix::stdio::stdin()) {
        return read_password_from(prompt, io::stdin().lock());
    }

    eprint!("{prompt}");
    io::stderr().flush()?;

    use crossterm::event::{Event, KeyCode, KeyModifiers, read};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    enable_raw_mode()?;
    let mut password = String::new();
    let res = loop {
        match read() {
            Ok(Event::Key(key_event)) => match key_event.code {
                KeyCode::Enter => {
                    eprintln!();
                    break Ok(password);
                }
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    eprintln!();
                    break Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                }
                KeyCode::Char(c) => password.push(c),
                KeyCode::Backspace => {
                    password.pop();
                }
                KeyCode::Esc => {
                    eprintln!();
                    break Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                }
                _ => {}
            },
            Ok(_) => {}
            Err(e) => {
                eprintln!();
                break Err(e);
            }
        }
    };
    let _ = disable_raw_mode();
    res
}

/// Read a line from a reader, trimming trailing newline.
pub fn read_password_from<R: BufRead>(prompt: &str, mut reader: R) -> io::Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn non_tty_stdin_reads_line() {
        let cursor = Cursor::new(b"secret_password\n");
        let result = read_password_from("Password: ", cursor).unwrap();
        assert_eq!(result, "secret_password");
    }
}
