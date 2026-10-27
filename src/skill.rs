use crate::{
    error::{Result, message},
    output::Output,
};
use std::io::Write;
pub const SKILL: &str = include_str!("../skills/remarkable/SKILL.md");
include!(concat!(env!("OUT_DIR"), "/references.rs"));
pub fn topics() -> Vec<&'static str> {
    REFERENCES.iter().map(|(topic, _)| *topic).collect()
}
pub fn commands(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.strip_prefix("## `").and_then(|s| s.strip_suffix('`')))
        .collect()
}
pub fn print(output: &Output, out: &mut dyn Write, topic: Option<&str>, list: bool) -> Result<()> {
    if list {
        let rows = REFERENCES
            .iter()
            .flat_map(|(topic, text)| {
                commands(text)
                    .into_iter()
                    .map(|cmd| vec![topic.to_string(), cmd.to_string()])
            })
            .collect::<Vec<_>>();
        output.table(out, &["TOPIC", "COMMAND"], &rows)?;
    } else {
        out.write_all(
            match topic {
                None => SKILL,
                Some(topic) => REFERENCES
                    .iter()
                    .find(|(name, _)| *name == topic)
                    .map(|(_, text)| *text)
                    .ok_or_else(|| message("unknown skill reference"))?,
            }
            .as_bytes(),
        )?;
    }
    Ok(())
}
