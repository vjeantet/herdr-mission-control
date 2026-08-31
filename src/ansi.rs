//! Minimal SGR parser: herdr's `pane read --format ansi` regenerates its
//! output from the cell grid, so it only ever contains text, newlines and
//! SGR sequences (verified against src/pane/terminal.rs and live output).
//! Anything else escape-like is dropped.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn parse(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut style = Style::default();
    let mut chars = text.chars().peekable();

    let flush = |current: &mut String, spans: &mut Vec<Span<'static>>, style: Style| {
        if !current.is_empty() {
            spans.push(Span::styled(std::mem::take(current), style));
        }
    };

    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    let mut params = String::new();
                    let mut terminator = None;
                    for c in chars.by_ref() {
                        if c.is_ascii_digit() || c == ';' || c == ':' {
                            params.push(c);
                        } else {
                            terminator = Some(c);
                            break;
                        }
                    }
                    if terminator == Some('m') {
                        flush(&mut current, &mut spans, style);
                        style = apply_sgr(style, &params);
                    }
                }
                // Non-CSI escapes do not occur in herdr output; ignore.
            }
            '\r' => {}
            '\n' => {
                flush(&mut current, &mut spans, style);
                lines.push(Line::from(std::mem::take(&mut spans)));
            }
            _ => current.push(c),
        }
    }
    flush(&mut current, &mut spans, style);
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn apply_sgr(mut style: Style, params: &str) -> Style {
    let codes: Vec<u16> = params
        .split([';', ':'])
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let codes = if codes.is_empty() { vec![0] } else { codes };

    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => {
                style = style
                    .remove_modifier(Modifier::BOLD)
                    .remove_modifier(Modifier::DIM)
            }
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style = style.fg(base_color(codes[i] - 30)),
            39 => style = style.fg(Color::Reset),
            40..=47 => style = style.bg(base_color(codes[i] - 40)),
            49 => style = style.bg(Color::Reset),
            90..=97 => style = style.fg(bright_color(codes[i] - 90)),
            100..=107 => style = style.bg(bright_color(codes[i] - 100)),
            38 | 48 => {
                let is_fg = codes[i] == 38;
                let color = match codes.get(i + 1) {
                    Some(5) => {
                        let c = codes.get(i + 2).copied().unwrap_or(0);
                        i += 2;
                        Some(Color::Indexed(c as u8))
                    }
                    Some(2) => {
                        let r = codes.get(i + 2).copied().unwrap_or(0) as u8;
                        let g = codes.get(i + 3).copied().unwrap_or(0) as u8;
                        let b = codes.get(i + 4).copied().unwrap_or(0) as u8;
                        i += 4;
                        Some(Color::Rgb(r, g, b))
                    }
                    _ => None,
                };
                if let Some(color) = color {
                    style = if is_fg { style.fg(color) } else { style.bg(color) };
                }
            }
            _ => {}
        }
        i += 1;
    }
    style
}

fn base_color(index: u16) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

fn bright_color(index: u16) -> Color {
    match index {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn plain_text_splits_lines() {
        let lines = parse("hello\r\nworld");
        assert_eq!(lines.len(), 2);
        assert_eq!(text_of(&lines[0]), "hello");
        assert_eq!(text_of(&lines[1]), "world");
    }

    #[test]
    fn sgr_colors_apply_and_reset() {
        let lines = parse("\x1b[31;1mred\x1b[0m plain");
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        assert_eq!(spans[0].content, "red");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[1].content, " plain");
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn rgb_and_indexed_colors() {
        let lines = parse("\x1b[38;2;153;153;153mgrey\x1b[38;5;12mblue");
        let spans = &lines[0].spans;
        assert_eq!(spans[0].style.fg, Some(Color::Rgb(153, 153, 153)));
        assert_eq!(spans[1].style.fg, Some(Color::Indexed(12)));
    }

    #[test]
    fn unterminated_escape_is_dropped() {
        let lines = parse("ok\x1b[38;2;1");
        assert_eq!(text_of(&lines[0]), "ok");
    }
}
