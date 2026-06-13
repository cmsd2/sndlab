//! Ring-buffer log pane state. Append-only from the app's perspective,
//! bounded so we never grow unbounded over a long session.

// Helpers get wired up as the engine/MCP layers land.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Instant;

const MAX_LINES: usize = 256;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub when: Instant,
    pub line: String,
    pub kind: LogKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Warn,
    Error,
    Audio,
}

#[derive(Debug, Default)]
pub struct LogPane {
    entries: VecDeque<LogEntry>,
}

impl LogPane {
    pub fn push(&mut self, kind: LogKind, line: impl Into<String>) {
        if self.entries.len() == MAX_LINES {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            when: Instant::now(),
            line: line.into(),
            kind,
        });
    }

    pub fn info(&mut self, line: impl Into<String>) {
        self.push(LogKind::Info, line);
    }
    pub fn warn(&mut self, line: impl Into<String>) {
        self.push(LogKind::Warn, line);
    }
    pub fn error(&mut self, line: impl Into<String>) {
        self.push(LogKind::Error, line);
    }
    pub fn audio(&mut self, line: impl Into<String>) {
        self.push(LogKind::Audio, line);
    }

    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }
}
