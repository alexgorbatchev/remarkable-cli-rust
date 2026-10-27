use cli_command_kit::Mode;
use std::{
    io::{self, Write},
    path::Path,
};
use unicode_general_category::{GeneralCategory as G, get_general_category};

pub struct Output {
    pub mode: Mode,
}
impl Output {
    pub fn new(mode: Mode) -> Self {
        Self { mode }
    }
    pub fn path(&self, path: &Path) -> String {
        cli_command_kit::display_path(path)
    }
    pub fn status(&self, out: &mut dyn Write, level: &str, message: &str) -> io::Result<()> {
        let upper = level.trim().to_ascii_uppercase();
        if self.mode == Mode::Agent {
            let prefix = match upper.as_str() {
                "ERROR" => "ERR",
                "WARNING" => "WARN",
                _ => &upper,
            };
            writeln!(out, "{prefix}: {message}")
        } else {
            writeln!(out, "{:<7} {message}", format!("[{upper}]"))
        }
    }
    pub fn table(
        &self,
        out: &mut dyn Write,
        headers: &[&str],
        rows: &[Vec<String>],
    ) -> io::Result<()> {
        let escaped: Vec<Vec<_>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| printable_text(cell.as_bytes()))
                    .collect()
            })
            .collect();
        if self.mode == Mode::Agent {
            if !headers.is_empty() {
                writeln!(out, "{}", headers.join("\t"))?;
            }
            for row in escaped {
                writeln!(out, "{}", row.join("\t"))?;
            }
            Ok(())
        } else {
            let mut table = comfy_table::Table::new();
            table.load_preset(comfy_table::presets::ASCII_FULL);
            if !headers.is_empty() {
                table.set_header(headers);
            }
            for row in escaped {
                table.add_row(row);
            }
            writeln!(out, "{table}")
        }
    }
    pub fn key_values(&self, out: &mut dyn Write, pairs: &[(&str, String)]) -> io::Result<()> {
        let width = pairs
            .iter()
            .map(|(key, _)| key.len() + 1)
            .max()
            .unwrap_or(0);
        for (key, value) in pairs {
            let value = printable_text(value.as_bytes());
            if self.mode == Mode::Agent {
                writeln!(out, "{key}: {value}")?;
            } else {
                writeln!(out, "{:<width$}  {value}", format!("{key}:"))?;
            }
        }
        Ok(())
    }
    pub fn tree(&self, out: &mut dyn Write, root: &TreeNode) -> io::Result<()> {
        if self.mode == Mode::Agent {
            agent_tree(out, root, 0)
        } else {
            writeln!(out, "{}", printable_text(root.label.as_bytes()))?;
            human_children(out, &root.children, "")
        }
    }
    pub fn separator(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.mode == Mode::Human {
            writeln!(
                out,
                "{}",
                "-".repeat(match cli_command_kit::terminal_width() {
                    0 => 80,
                    width => width.min(120),
                })
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct TreeNode {
    pub label: String,
    pub children: Vec<TreeNode>,
}
fn agent_tree(out: &mut dyn Write, node: &TreeNode, depth: usize) -> io::Result<()> {
    writeln!(
        out,
        "{}* {}",
        "  ".repeat(depth),
        printable_text(node.label.as_bytes())
    )?;
    for child in &node.children {
        agent_tree(out, child, depth + 1)?;
    }
    Ok(())
}
fn human_children(out: &mut dyn Write, nodes: &[TreeNode], prefix: &str) -> io::Result<()> {
    for (i, node) in nodes.iter().enumerate() {
        let last = i + 1 == nodes.len();
        writeln!(
            out,
            "{prefix}{}{}",
            if last { "└── " } else { "├── " },
            printable_text(node.label.as_bytes())
        )?;
        human_children(
            out,
            &node.children,
            &format!("{prefix}{}", if last { "    " } else { "│   " }),
        )?;
    }
    Ok(())
}

pub fn printable(ch: char) -> bool {
    ch == ' '
        || !matches!(
            get_general_category(ch),
            G::Control
                | G::Format
                | G::Unassigned
                | G::PrivateUse
                | G::Surrogate
                | G::SpaceSeparator
                | G::LineSeparator
                | G::ParagraphSeparator
        )
}
fn escaped(ch: char) -> String {
    match ch {
        '\\' => "\\\\".into(),
        '\u{7}' => "\\a".into(),
        '\u{8}' => "\\b".into(),
        '\u{c}' => "\\f".into(),
        '\n' => "\\n".into(),
        '\r' => "\\r".into(),
        '\t' => "\\t".into(),
        '\u{b}' => "\\v".into(),
        ch if (ch as u32) < 0x80 => format!("\\x{:02x}", ch as u32),
        ch if (ch as u32) <= 0xffff => format!("\\u{:04x}", ch as u32),
        ch => format!("\\U{:08x}", ch as u32),
    }
}
fn display_bytes(bytes: &[u8], uri: bool) -> String {
    let mut out = String::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        let (valid, invalid) = match std::str::from_utf8(remaining) {
            Ok(text) => (text, 0),
            Err(error) => (
                std::str::from_utf8(&remaining[..error.valid_up_to()])
                    .expect("validated UTF-8 prefix"),
                error
                    .error_len()
                    .unwrap_or(remaining.len() - error.valid_up_to()),
            ),
        };
        for ch in valid.chars() {
            if printable(ch) && if uri { ch != ' ' } else { ch != '\\' } {
                out.push(ch);
            } else if uri {
                let mut encoded = [0; 4];
                for byte in ch.encode_utf8(&mut encoded).bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            } else {
                out.push_str(&escaped(ch));
            }
        }
        for byte in &remaining[valid.len()..valid.len() + invalid] {
            out.push_str(&if uri {
                format!("%{byte:02X}")
            } else {
                format!("\\x{byte:02x}")
            });
        }
        remaining = &remaining[valid.len() + invalid..];
    }
    out
}
pub fn printable_text(bytes: &[u8]) -> String {
    display_bytes(bytes, false)
}
pub fn printable_uri(bytes: &[u8]) -> String {
    display_bytes(bytes, true)
}
