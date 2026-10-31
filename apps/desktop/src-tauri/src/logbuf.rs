//! Keeps the most recent warnings and errors in memory, so the debug report
//! can include them without AgentScope writing a log file.

use std::collections::VecDeque;
use std::io;
use std::sync::Mutex;

use tracing_subscriber::fmt::MakeWriter;

/// How many lines the report shows.
const CAPACITY: usize = 50;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

fn push(line: &str) {
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if lines.len() == CAPACITY {
        lines.pop_front();
    }
    lines.push_back(line.to_owned());
}

/// The kept lines, oldest first.
pub fn recent() -> Vec<String> {
    LINES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// A `tracing` writer that stores each formatted event as one line.
#[derive(Clone, Copy, Default)]
pub struct RecentLogs;

pub struct LineWriter(Vec<u8>);

impl io::Write for LineWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        push(&String::from_utf8_lossy(&self.0));
    }
}

impl<'a> MakeWriter<'a> for RecentLogs {
    type Writer = LineWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LineWriter(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_lines() {
        for i in 0..CAPACITY + 5 {
            let mut w = RecentLogs.make_writer();
            io::Write::write_all(&mut w, format!("line {i}\n").as_bytes()).unwrap();
        }
        let lines = recent();
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines.last().unwrap(), &format!("line {}", CAPACITY + 4));
        assert_eq!(lines[0], "line 5");
    }
}
