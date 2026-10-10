//! The bridge's, engine's and adapter's own output (E05). Each role's
//! standard output and error share one private pipe; a thread copies each
//! line to the supervisor's standard error, which is the unit's journal,
//! prefixed with the role (`engine: ...`). The application keeps its
//! inherited standard error (the bounded failed-helper record).
//!
//! The copy is bounded so that a role cannot crowd out the supervisor's own
//! records or reach the journal's rate limit for the unit: a line is cut at
//! `MAX_LINE_BYTES`, each role sends at most `LINES_PER_WINDOW` lines in
//! each `WINDOW` and counts the rest, and control characters are replaced.
//! It always drains the pipe, so a role never blocks on its own output, even
//! when the journal refuses a write.
use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

pub const MAX_LINE_BYTES: usize = 4096;
pub const WINDOW: Duration = Duration::from_secs(30);
pub const LINES_PER_WINDOW: usize = 1000;

/// The copy of one role's output, until the role closes it.
pub struct Relay<W, C> {
    role: &'static str,
    out: W,
    now: C,
    lines_per_window: usize,
    window: Duration,
    started: Instant,
    sent: usize,
    dropped: u64,
    line: Vec<u8>,
    cut: bool,
}

impl<W: Write, C: FnMut() -> Instant> Relay<W, C> {
    pub fn new(role: &'static str, out: W, mut now: C, lines_per_window: usize, window: Duration) -> Self {
        let started = now();
        Self { role, out, now, lines_per_window, window, started, sent: 0, dropped: 0,
            line: Vec::with_capacity(256), cut: false }
    }

    /// Copies until end of input or a read error.
    pub fn run(mut self, mut input: impl Read) {
        let mut buffer = [0u8; 8192];
        loop {
            let count = match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            for &byte in &buffer[..count] {
                if byte == b'\n' {
                    self.line_end();
                } else if self.line.len() < MAX_LINE_BYTES {
                    self.line.push(byte);
                } else {
                    self.cut = true;
                }
            }
        }
        if !self.line.is_empty() || self.cut {
            self.line_end();
        }
        self.report_dropped();
    }

    fn line_end(&mut self) {
        let now = (self.now)();
        if now.saturating_duration_since(self.started) >= self.window {
            self.report_dropped();
            self.started = now;
            self.sent = 0;
        }
        if self.sent < self.lines_per_window {
            self.sent += 1;
            let mut text = format!("{}: {}", self.role, clean(&self.line));
            if self.cut {
                text.push_str(" [cut at 4096 bytes]");
            }
            self.write(text);
        } else {
            self.dropped += 1;
        }
        self.line.clear();
        self.cut = false;
    }

    fn report_dropped(&mut self) {
        if self.dropped > 0 {
            let text = format!("{}: [{} lines dropped over the limit of {} in {} s]", self.role, self.dropped,
                self.lines_per_window, self.window.as_secs());
            self.write(text);
            self.dropped = 0;
        }
    }

    /// One write per line, so lines from different roles never interleave.
    /// A refused write is not retried: the pipe keeps draining.
    fn write(&mut self, mut text: String) {
        text.push('\n');
        let _ = self.out.write_all(text.as_bytes()).and_then(|()| self.out.flush());
    }
}

/// The line as text: invalid UTF-8 replaced, a trailing carriage return
/// dropped, and other control characters except tab shown as `?`.
fn clean(line: &[u8]) -> String {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    String::from_utf8_lossy(line)
        .chars()
        .map(|c| if c.is_control() && c != '\t' { '?' } else { c })
        .collect()
}

/// Starts copying a role's output from the read end of its pipe to this
/// process's standard error.
pub fn start(role: &'static str, input: std::fs::File) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name(format!("{role}-log"))
        .spawn(move || {
            Relay::new(role, StderrLines, Instant::now, LINES_PER_WINDOW, WINDOW).run(input)
        })
}

/// The process's standard error, locked for each line.
struct StderrLines;
impl Write for StderrLines {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        std::io::stderr().lock().write(bytes)
    }
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        std::io::stderr().lock().write_all(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().lock().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn relay(input: &[u8], lines: usize, step: Duration) -> String {
        let base = Instant::now();
        let ticks = Rc::new(Cell::new(0u32));
        let clock = {
            let ticks = ticks.clone();
            move || {
                let at = base + step * ticks.get();
                ticks.set(ticks.get() + 1);
                at
            }
        };
        let mut out = Vec::new();
        Relay::new("engine", &mut out, clock, lines, Duration::from_secs(30)).run(input);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn each_line_is_prefixed_with_its_role() {
        let out = relay(b"{\"level\":\"info\",\"msg\":\"committed state\"}\npanic: x\r\nlast", 10, Duration::ZERO);
        assert_eq!(out, "engine: {\"level\":\"info\",\"msg\":\"committed state\"}\nengine: panic: x\nengine: last\n");
    }

    #[test]
    fn long_lines_are_cut_and_control_characters_replaced() {
        let mut input = vec![b'a'; MAX_LINE_BYTES + 10];
        input.extend_from_slice(b"\nbell\x07\tand \xff\n");
        let out = relay(&input, 10, Duration::ZERO);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines[0], format!("engine: {} [cut at 4096 bytes]", "a".repeat(MAX_LINE_BYTES)));
        assert_eq!(lines[1], "engine: bell?\tand \u{fffd}");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn lines_over_the_budget_are_counted_not_sent() {
        // Line n ends at 10n s: windows start at 0, 30 and 60 s.
        let input: Vec<u8> = (1..=8).flat_map(|n| format!("line {n}\n").into_bytes()).collect();
        let out = relay(&input, 2, Duration::from_secs(10));
        assert_eq!(out, concat!(
            "engine: line 1\nengine: line 2\n",
            "engine: line 3\nengine: line 4\n",
            "engine: [1 lines dropped over the limit of 2 in 30 s]\n",
            "engine: line 6\nengine: line 7\n",
            "engine: [1 lines dropped over the limit of 2 in 30 s]\n"));
    }

    #[test]
    fn drops_are_reported_when_the_output_closes() {
        let out = relay(b"a\nb\nc\nd\n", 1, Duration::ZERO);
        assert_eq!(out, "engine: a\nengine: [3 lines dropped over the limit of 1 in 30 s]\n");
    }

    /// A journal that refuses every write still drains the input.
    #[test]
    fn a_refused_write_keeps_draining() {
        struct Refusing(usize);
        impl Write for Refusing {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                self.0 += 1;
                Err(std::io::Error::other("journal gone"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let input = vec![b'x'; 100_000].into_iter().chain(b"\n".repeat(50)).collect::<Vec<_>>();
        let mut refusing = Refusing(0);
        let mut slice = input.as_slice();
        Relay::new("bridge", &mut refusing, Instant::now, 10, WINDOW).run(&mut slice);
        assert!(slice.is_empty());
        // Ten lines and the drop notice were offered; none was retried.
        assert_eq!(refusing.0, 11);
    }
}
