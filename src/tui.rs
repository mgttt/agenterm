//! Placeholder terminal UI for the unified `agenterm tui` entry point.

use std::io;

use agenterm_platform::console_line_editor::{ConsoleKey, ConsoleLineEditor};

const HELP: &str = "\
Usage: agenterm tui
       agenterm tui --help
       agenterm tui --version

Open the placeholder AgenTerm terminal UI. Press q or Enter to return.";

const FRAME: &str = "\
\x1b[2J\x1b[H\
\x1b[1;38;5;45m+----------------------------------------------------------+\x1b[0m\r\n\
\x1b[1;38;5;45m|\x1b[0m                    \x1b[1mAGENTERM TUI\x1b[0m                      \x1b[1;38;5;45m|\x1b[0m\r\n\
\x1b[1;38;5;45m+----------------------------------------------------------+\x1b[0m\r\n\
\r\n\
  Placeholder workspace\r\n\
\r\n\
  The interactive workspace will grow here in later releases.\r\n\
  This first surface proves the unified executable, terminal mode,\r\n\
  keyboard input, and clean return to the calling shell.\r\n\
\r\n\
  \x1b[38;5;244mPress q or Enter to return.\x1b[0m\r\n";

pub fn run_entry_with_args(args: Vec<String>) -> i32 {
    match args.as_slice() {
        [] => match run_placeholder() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("agenterm tui: {error}");
                1
            }
        },
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}");
            0
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("agenterm tui {}", env!("CARGO_PKG_VERSION"));
            0
        }
        [unknown, ..] => {
            eprintln!("agenterm tui: unexpected argument '{unknown}'\n\n{HELP}");
            2
        }
    }
}

fn run_placeholder() -> Result<(), String> {
    let mut editor = ConsoleLineEditor::enter().map_err(|error| error.to_string())?;
    let stdout = io::stdout();
    let mut screen = AlternateScreen::enter(stdout.lock()).map_err(|error| error.to_string())?;
    screen.render().map_err(|error| error.to_string())?;
    loop {
        match editor.read_key().map_err(|error| error.to_string())? {
            Some(key) if exits_tui(key) => break,
            Some(_) | None => {}
        }
    }
    drop(screen);
    drop(editor);
    Ok(())
}

fn exits_tui(key: ConsoleKey) -> bool {
    matches!(
        key,
        ConsoleKey::Char('q' | 'Q') | ConsoleKey::Enter | ConsoleKey::Eof
    )
}

struct AlternateScreen<W: io::Write> {
    writer: W,
}

impl<W: io::Write> AlternateScreen<W> {
    fn enter(mut writer: W) -> io::Result<Self> {
        writer.write_all(b"\x1b[?1049h\x1b[?25l")?;
        writer.flush()?;
        Ok(Self { writer })
    }

    fn render(&mut self) -> io::Result<()> {
        self.writer.write_all(FRAME.as_bytes())?;
        self.writer.flush()
    }
}

impl<W: io::Write> Drop for AlternateScreen<W> {
    fn drop(&mut self) {
        let _ = self.writer.write_all(b"\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_exit_keys_close_the_placeholder() {
        assert!(exits_tui(ConsoleKey::Char('q')));
        assert!(exits_tui(ConsoleKey::Char('Q')));
        assert!(exits_tui(ConsoleKey::Enter));
        assert!(exits_tui(ConsoleKey::Eof));
        assert!(!exits_tui(ConsoleKey::Char('x')));
        assert!(!exits_tui(ConsoleKey::Left));
    }

    #[test]
    fn screen_guard_enters_renders_and_restores_terminal_state() {
        let mut bytes = Vec::new();
        {
            let mut screen = AlternateScreen::enter(&mut bytes).unwrap();
            screen.render().unwrap();
        }
        assert!(bytes.starts_with(b"\x1b[?1049h\x1b[?25l"));
        assert!(
            bytes
                .windows(b"AGENTERM TUI".len())
                .any(|part| part == b"AGENTERM TUI")
        );
        assert!(bytes.ends_with(b"\x1b[0m\x1b[?25h\x1b[?1049l"));
    }
}
