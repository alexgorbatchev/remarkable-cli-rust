use clap::Command;
use cli_command_kit::Mode;
use remarkable_cli::{commands, skill};
use std::collections::{BTreeMap, BTreeSet};

fn sections(text: &str) -> Vec<(&str, &str)> {
    let mut result = Vec::new();
    for part in text.split("\n## `").skip(1) {
        let (heading, body) = part.split_once("`\n").expect("command heading");
        result.push((heading, body));
    }
    result
}

fn check(
    command: &Command,
    path: &str,
    sections: &BTreeMap<String, (&str, &str)>,
    seen: &mut BTreeSet<String>,
) {
    let (heading, body) = sections
        .get(path)
        .unwrap_or_else(|| panic!("missing documentation for {path}"));
    assert!(seen.insert(path.to_string()), "duplicate command {path}");
    // Clap implements generated `help [command]` by traversing its command tree,
    // rather than exposing the path as a positional Arg.
    if path == "remarkable help" {
        let error = commands::command(Mode::Agent)
            .try_get_matches_from(["remarkable", "help", "doc", "render"])
            .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(error.to_string().starts_with(commands::ALERT));
        return;
    }
    let positional = command.get_positionals().collect::<Vec<_>>();
    let documented = heading
        .split_whitespace()
        .filter(|s| s.starts_with('<') || s.starts_with('['))
        .collect::<Vec<_>>();
    assert_eq!(positional.len(), documented.len(), "{heading}");
    for (arg, usage) in positional.iter().zip(documented) {
        let name = usage.trim_matches(['<', '>', '[', ']']);
        assert_eq!(arg.get_id().as_str(), name, "{heading}");
        assert_eq!(arg.is_required_set(), usage.starts_with('<'), "{heading}");
    }
    for arg in command
        .get_arguments()
        .filter(|arg| !arg.is_positional() && (!arg.is_global_set() || path == "remarkable"))
    {
        let long = arg.get_long().expect("long flag");
        let table = if ["help", "version"].contains(&long) {
            sections["remarkable"].1
        } else {
            body
        };
        let row = table
            .lines()
            .find(|line| line.starts_with(&format!("| `--{long}` |")))
            .unwrap_or_else(|| panic!("undocumented flag {path} --{long}"));
        let cells = row.split('|').map(str::trim).collect::<Vec<_>>();
        if let Some(short) = arg.get_short() {
            assert_eq!(cells[2], format!("`-{short}`"), "{path} --{long}");
        } else {
            assert_eq!(cells[2], "—", "{path} --{long}");
        }
        if let Some(default) = arg.get_default_values().first() {
            let expected = if default.is_empty() {
                "\"\"".to_string()
            } else {
                default.to_string_lossy().into_owned()
            };
            assert_eq!(cells[4], format!("`{expected}`"), "{path} --{long}");
        }
        let native = arg.get_value_parser().type_id();
        let expected = if matches!(
            arg.get_action(),
            clap::ArgAction::Help
                | clap::ArgAction::HelpShort
                | clap::ArgAction::HelpLong
                | clap::ArgAction::Version
                | clap::ArgAction::SetTrue
                | clap::ArgAction::SetFalse
        ) {
            "`bool`"
        } else if native == std::any::TypeId::of::<String>() {
            "`string`"
        } else if native == std::any::TypeId::of::<bool>() {
            "`bool`"
        } else if native == std::any::TypeId::of::<f64>() {
            "`float64`"
        } else if native == std::any::TypeId::of::<usize>()
            || native == std::any::TypeId::of::<i32>()
            || native == std::any::TypeId::of::<i64>()
        {
            "`int`"
        } else {
            panic!("undocumented native parser type for {path} --{long}: {native:?}")
        };
        assert_eq!(cells[3], expected, "{path} --{long}");
    }
    for child in command.get_subcommands() {
        check(
            child,
            &format!("{path} {}", child.get_name()),
            sections,
            seen,
        );
    }
}

#[test]
fn embedded_contract_covers_every_live_command_argument_flag_default_and_topic() {
    let mut docs = sections(skill::SKILL);
    for (_, text) in skill::REFERENCES {
        assert!(text.lines().count() <= 100);
        docs.extend(sections(text));
    }
    assert!(skill::SKILL.lines().count() <= 499);
    let mut by_path = BTreeMap::new();
    for (heading, body) in docs {
        let path = heading
            .split_whitespace()
            .take_while(|s| !s.starts_with('<') && !s.starts_with('['))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            by_path.insert(path, (heading, body)).is_none(),
            "duplicate heading {heading}"
        );
    }
    let mut command = commands::command(Mode::Agent);
    command.build();
    let mut seen = BTreeSet::new();
    check(&command, "remarkable", &by_path, &mut seen);
    assert_eq!(seen, by_path.keys().cloned().collect());
    for (topic, text) in skill::REFERENCES {
        let prefix = format!("| `AGENT=1 remarkable skill reference cat {topic}` | ");
        let rows = skill::SKILL
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1, "index for {topic}");
        let commands = rows[0]
            .split_once(": ")
            .expect("index purpose")
            .0
            .split('`')
            .enumerate()
            .filter_map(|(i, text)| (i % 2 == 1).then_some(text))
            .collect::<Vec<_>>();
        let headings = skill::commands(text)
            .into_iter()
            .map(|heading| {
                let command = heading
                    .split(" <")
                    .next()
                    .unwrap()
                    .split(" [")
                    .next()
                    .unwrap();
                command.strip_prefix("remarkable ").unwrap_or(command)
            })
            .collect::<Vec<_>>();
        assert_eq!(commands, headings, "index command order for {topic}");
    }
    for (_, text) in
        std::iter::once(("skill", skill::SKILL)).chain(skill::REFERENCES.iter().copied())
    {
        for mention in text.split("remarkable skill reference cat ").skip(1) {
            let topic = mention.split(['`', ' ', '\n']).next().unwrap();
            if topic == "<topic>" {
                continue;
            }
            assert!(skill::topics().contains(&topic), "unknown topic {topic}");
        }
    }
}
