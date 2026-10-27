use crate::validation as v;
use clap::{
    Arg, ArgAction, Command,
    builder::{StringValueParser, TypedValueParser},
};
use cli_command_kit::{HelpOptions, Mode};
pub const ALERT: &str =
    "ALERT: Agents must read `AGENT=1 remarkable skill` before using this tool.";
#[derive(Clone)]
struct UsageParser<P>(P);
impl<P: TypedValueParser> TypedValueParser for UsageParser<P> {
    type Value = P::Value;
    fn parse_ref(
        &self,
        command: &Command,
        arg: Option<&Arg>,
        value: &std::ffi::OsStr,
    ) -> std::result::Result<Self::Value, clap::Error> {
        self.0.parse_ref(command, arg, value).map_err(|mut error| {
            error.insert(
                clap::error::ContextKind::Usage,
                clap::error::ContextValue::StyledStr(command.clone().render_usage()),
            );
            error
        })
    }
}
fn validated<T: Send + Sync + Clone + 'static>(
    parse: fn(&str) -> std::result::Result<T, String>,
) -> impl TypedValueParser<Value = T> {
    UsageParser(StringValueParser::new().try_map(move |value: String| parse(&value)))
}
fn group(name: &str, about: &str) -> Command {
    Command::new(name.to_owned())
        .about(about.to_owned())
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
}
fn positional(name: &str, parser: fn(&str) -> std::result::Result<String, String>) -> Arg {
    Arg::new(name.to_owned())
        .required(true)
        .value_parser(validated(parser))
}
fn text(name: &str, default: &str, about: &str) -> Arg {
    Arg::new(name.to_owned())
        .long(name.to_owned())
        .default_value(default.to_owned())
        .help(about.to_owned())
}
fn checked(
    name: &str,
    parser: fn(&str) -> std::result::Result<String, String>,
    about: &str,
) -> Arg {
    Arg::new(name.to_owned())
        .long(name.to_owned())
        .value_parser(validated(parser))
        .help(about.to_owned())
}
fn boolean(name: &str, about: &str) -> Arg {
    Arg::new(name.to_owned())
        .long(name.to_owned())
        .action(ArgAction::Set)
        .num_args(0..=1)
        .require_equals(true)
        .default_missing_value("true")
        .default_value("false")
        .value_parser(UsageParser(clap::builder::BoolValueParser::new()))
        .help(about.to_owned())
}
fn page() -> Arg {
    text("page", "0", "Page index (0-based)").value_parser(validated(v::index))
}
fn dpi() -> Arg {
    text("dpi", "200", "Rendering resolution DPI")
        .allow_hyphen_values(true)
        .value_parser(UsageParser(clap::value_parser!(i32)))
}
fn id() -> Arg {
    positional("id-or-name", |s| Ok(s.into()))
}
pub fn raw_command() -> Command {
    let auth = group("auth", "Manage cloud authentication and device pairing")
        .subcommand(
            Command::new("pair")
                .about("Pair a device using an eight-character code")
                .arg(positional("code", v::pairing_code)),
        )
        .subcommand(Command::new("status").about("Check the cloud connection"))
        .subcommand(Command::new("token").about("Print the current bearer user token"));
    let doc = group("doc", "Browse, export, and update documents")
        .subcommand(
            Command::new("list")
                .about("List cloud documents and folders")
                .arg(text("folder", "", "Filter by parent folder ID"))
                .arg(text("type", "", "Filter by item type"))
                .arg(text("query", "", "Filter title substring").short('q'))
                .arg(
                    text("limit", "0", "Maximum number of items")
                        .allow_hyphen_values(true)
                        .value_parser(UsageParser(clap::value_parser!(i64))),
                ),
        )
        .subcommand(Command::new("tree").about("Display the document hierarchy"))
        .subcommand(
            Command::new("inspect")
                .about("Display metadata and native pages")
                .arg(id())
                .arg(boolean("pages", "Include native page IDs and stroke sizes")),
        )
        .subcommand(
            Command::new("search")
                .about("Search PDF page text")
                .arg(id())
                .arg(positional("query", v::query))
                .arg(boolean("word", "Match whole words")),
        )
        .subcommand(
            Command::new("links")
                .about("List page hyperlinks")
                .arg(id())
                .arg(page()),
        )
        .subcommand(
            Command::new("cat")
                .about("Stream PDF, text, RM, or SVG bytes")
                .arg(id())
                .arg(page())
                .arg(
                    text("format", "svg", "Output format: pdf, text, rm, svg")
                        .value_parser(validated(v::cat_format)),
                ),
        )
        .subcommand(
            Command::new("render")
                .about("Render a page with strokes to PNG")
                .arg(id())
                .arg(page())
                .arg(dpi())
                .arg(
                    checked("output", v::nonempty, "Destination PNG path")
                        .short('o')
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("sync")
                .about("Export document pages to a directory")
                .arg(id())
                .arg(
                    text("output-dir", ".", "Export directory")
                        .short('o')
                        .value_parser(validated(v::nonempty)),
                )
                .arg(
                    text("format", "png", "Page format: png, svg, rm")
                        .value_parser(validated(v::sync_format)),
                )
                .arg(dpi())
                .arg(boolean("force", "Overwrite existing exports").short('f')),
        )
        .subcommand(
            Command::new("archive")
                .about("Save a fresh, verified native ZIP snapshot")
                .arg(id())
                .arg(
                    checked("output", v::nonempty, "New archive path")
                        .short('o')
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("import")
                .about("Import local RM pages into native pages")
                .arg(positional("destination-uuid", v::uuid))
                .arg(checked("mapping", v::nonempty, "JSON source/page mapping").required(true)),
        )
        .subcommand(
            Command::new("upload")
                .about("Upload a PDF as a separate document")
                .arg(positional("pdf", v::nonempty))
                .arg(checked("title", v::title, "Display title").required(true))
                .arg(
                    text("folder", "", "Destination folder UUID")
                        .value_parser(validated(v::folder)),
                )
                .arg(checked("evidence", v::nonempty, "New recovery JSON path").required(true))
                .arg(boolean(
                    "initialize-pages",
                    "Initialize up to 816 native page IDs",
                )),
        )
        .subcommand(
            Command::new("upload-check")
                .about("Verify saved upload evidence without retrying")
                .arg(positional("evidence", v::nonempty)),
        )
        .subcommand(
            group("settings", "Manage tags and view settings").subcommand(
                Command::new("transfer")
                    .about("Transfer mapped tags and view settings")
                    .arg(positional("source-uuid", v::uuid))
                    .arg(positional("destination-uuid", v::uuid))
                    .arg(checked("mapping", v::nonempty, "JSON page mapping").required(true))
                    .arg(boolean(
                        "replace-viewport",
                        "Replace differing view settings",
                    )),
            ),
        );
    let stroke = group("stroke", "Inspect and export native RM files")
        .subcommand(
            Command::new("inspect")
                .about("Inspect RM blocks, tools, and colors")
                .arg(positional("file.rm", v::nonempty)),
        )
        .subcommand(
            Command::new("export")
                .about("Export an RM file as SVG")
                .arg(positional("file.rm", v::nonempty))
                .arg(text("output", "", "Destination SVG path (default: stdout)").short('o'))
                .arg(
                    text("width", "0", "Canvas width in points")
                        .allow_hyphen_values(true)
                        .value_parser(UsageParser(
                            StringValueParser::new().try_map(|s: String| s.parse::<f64>()),
                        )),
                )
                .arg(
                    text("height", "0", "Canvas height in points")
                        .allow_hyphen_values(true)
                        .value_parser(UsageParser(
                            StringValueParser::new().try_map(|s: String| s.parse::<f64>()),
                        )),
                ),
        );
    let skill = Command::new("skill")
        .about("Print the embedded usage skill")
        .disable_help_subcommand(true)
        .subcommand(
            group("reference", "Read embedded command references")
                .subcommand(Command::new("list").about("List topics and documented commands"))
                .subcommand(Command::new("cat").about("Print a reference verbatim").arg(
                    Arg::new("topic").required(true).value_parser(UsageParser(
                        clap::builder::PossibleValuesParser::new(crate::skill::topics()),
                    )),
                )),
        );
    let completion = group("completion", "Generate shell completion scripts").subcommands(
        ["bash", "zsh", "fish", "powershell"].map(|shell| {
            Command::new(shell)
                .about(format!("Generate {shell} completions"))
                .arg(boolean(
                    "no-descriptions",
                    "Omit descriptions from completions",
                ))
        }),
    );
    Command::new("remarkable")
        .about("Cloud Sync v3, native handwriting, and PDF tools for reMarkable")
        .version("dev")
        .disable_version_flag(true)
        .arg(
            Arg::new("version")
                .long("version")
                .short('v')
                .action(ArgAction::SetTrue)
                .help("Print the raw version string"),
        )
        .arg(
            text("config", "", "Credentials path")
                .short('c')
                .global(true),
        )
        .arg(text("cache-dir", "", "Blob cache directory").global(true))
        .arg(boolean("debug", "Trace every HTTP attempt").global(true))
        .arg(boolean("no-cache", "Disable the blob cache").global(true))
        .subcommands([
            auth,
            doc,
            stroke,
            skill,
            completion,
            Command::new("status").about("Alias for auth status"),
        ])
}
pub fn command(mode: Mode) -> Command {
    let options = HelpOptions {
        tree: cli_command_kit::TreeOptions {
            hide_generated_commands: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut root =
        cli_command_kit::setup(raw_command(), mode, &options).expect("valid command tree");
    fn alert(command: &mut Command) {
        let help = command.render_long_help().to_string();
        *command = command.clone().override_help(format!("{ALERT}\n{help}"));
        for child in command.get_subcommands_mut() {
            alert(child);
        }
    }
    if mode == Mode::Agent {
        alert(&mut root)
    }
    root
}
