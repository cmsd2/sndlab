//! Syntect-powered syntax highlighting for the editor pane.
//!
//! syntect doesn't ship a Rhai grammar so we use the Rust definition
//! (close enough — keywords differ, comments and string literals
//! match, the visual texture lines up). When we outgrow the
//! approximation we'll ship our own `.sublime-syntax` for Rhai under
//! `book/assets/syntaxes/`.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        // base16-ocean.dark sits well against the default dark TUI
        // background and has decent colour separation between
        // keywords, literals, strings, and identifiers.
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| theme_set.themes.values().next().cloned().unwrap());
        Self { syntax_set, theme }
    }

    /// Highlight the buffer as a stack of styled `Line`s, one per
    /// source line. Returns owned `Line<'static>` so the result can
    /// be moved into the render frame.
    pub fn highlight(&self, buffer: &str) -> Vec<Line<'static>> {
        let syntax = self
            .syntax_set
            .find_syntax_by_extension("rs")
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let mut h = HighlightLines::new(syntax, &self.theme);

        let mut lines: Vec<Line<'static>> = Vec::new();
        // syntect wants newline-terminated lines for stateful parsing
        // (a string literal opened on line N must stay open across
        // line breaks). `LinesWithEndings` preserves the trailing
        // \n for parsing; we strip it before rendering.
        for raw in LinesWithEndings::from(buffer) {
            let ranges = match h.highlight_line(raw, &self.syntax_set) {
                Ok(r) => r,
                Err(_) => {
                    // Fall back to plain: render the raw line in default fg.
                    let s = raw.trim_end_matches('\n').to_string();
                    lines.push(Line::from(s));
                    continue;
                }
            };
            let spans: Vec<Span<'static>> = ranges
                .into_iter()
                .map(|(style, text)| {
                    let mut t = text.to_string();
                    if t.ends_with('\n') {
                        t.pop();
                    }
                    Span::styled(t, syntect_to_ratatui(style))
                })
                .collect();
            lines.push(Line::from(spans));
        }
        // Buffer might not end with \n; LinesWithEndings handles that.
        // If the buffer is entirely empty, produce one blank Line so
        // the cursor has a row to sit on.
        if lines.is_empty() {
            lines.push(Line::default());
        }
        lines
    }
}

fn syntect_to_ratatui(s: SyntectStyle) -> Style {
    let fg = s.foreground;
    Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
}
