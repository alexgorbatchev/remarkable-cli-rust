use crate::{
    archive, commands,
    config::Options,
    doc,
    error::{Error, Result, after_commit, message},
    import,
    output::Output,
    settings, upload,
};
use clap::ArgMatches;
use cli_command_kit::Mode;
use std::{
    cell::Cell,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};
pub struct CommitProgress(Cell<Option<remarkable_cloud::UpdateState>>);
impl Default for CommitProgress {
    fn default() -> Self {
        Self(Cell::new(None))
    }
}
impl CommitProgress {
    pub fn set(&self, state: remarkable_cloud::UpdateState) {
        self.0.set(Some(state));
    }
    pub fn get(&self) -> remarkable_cloud::UpdateState {
        self.0
            .get()
            .unwrap_or(remarkable_cloud::UpdateState::Staged)
    }
}
fn s<'a>(m: &'a ArgMatches, key: &str) -> &'a str {
    m.try_get_one::<String>(key)
        .ok()
        .flatten()
        .map_or("", String::as_str)
}
fn b(m: &ArgMatches, key: &str) -> bool {
    *m.get_one::<bool>(key).unwrap_or(&false)
}
fn page(m: &ArgMatches) -> usize {
    *m.get_one::<usize>("page").unwrap_or(&0)
}
fn dpi(m: &ArgMatches) -> i32 {
    *m.get_one::<i32>("dpi").unwrap_or(&200)
}
pub async fn execute(
    args: impl IntoIterator<Item = impl Into<std::ffi::OsString> + Clone>,
    mode: Mode,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut command = commands::command(mode);
    let matches = match command.clone().try_get_matches_from(args) {
        Ok(m) => m,
        Err(e) => {
            let help = matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            let result = if help {
                write!(out, "{e}")
            } else {
                report_parse(e, mode, err)
            };
            return if result.is_err() {
                1
            } else if help {
                0
            } else {
                1
            };
        }
    };
    if b(&matches, "version") {
        return if writeln!(out, "dev").is_ok() { 0 } else { 1 };
    }
    if matches.subcommand().is_none() {
        return if write!(out, "{}", command.render_long_help()).is_ok() {
            0
        } else {
            1
        };
    }
    if let Some(("doc", doc)) = matches.subcommand()
        && let Some(("settings", settings)) = doc.subcommand()
        && let Some(("transfer", transfer)) = settings.subcommand()
        && s(transfer, "source-uuid").eq_ignore_ascii_case(s(transfer, "destination-uuid"))
    {
        let mut error = clap::Error::raw(
            clap::error::ErrorKind::ValueValidation,
            "source and destination must be separate documents",
        );
        error.insert(
            clap::error::ContextKind::Usage,
            clap::error::ContextValue::StyledStr(command.render_usage()),
        );
        let _ = report_parse(error, mode, err);
        return 1;
    }
    let output = Output::new(mode);
    let options = Options {
        config: s(&matches, "config").into(),
        cache_dir: s(&matches, "cache-dir").into(),
        debug: b(&matches, "debug"),
        no_cache: b(&matches, "no-cache"),
    };
    if options.debug {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("remarkable_cloud::http=debug")
            .with_target(false)
            .without_time()
            .with_ansi(false)
            .with_writer(std::io::stderr)
            .try_init();
    }
    let seconds = timeout(&matches);
    let progress = CommitProgress::default();
    let operation = dispatch(&matches, &options, &output, out, &progress);
    let result = with_deadline(
        operation,
        &progress,
        Duration::from_secs(seconds),
        tokio::signal::ctrl_c(),
    )
    .await;
    match result {
        Ok(()) => 0,
        Err(mut error) => {
            if matches!(error, Error::Interrupted(_))
                && let Some(state) = progress.0.get()
                && let Err(write_error) =
                    output.key_values(out, &[("state", import::state(state).into())])
            {
                error = Error::Multiple(vec![error, after_commit(state, write_error.into())]);
            }
            let status = error.exit_status();
            let _ = report(&error, &output, err);
            status
        }
    }
}
fn report_parse(mut error: clap::Error, mode: Mode, out: &mut dyn Write) -> std::io::Result<()> {
    if let Some(usage) = error.remove(clap::error::ContextKind::Usage) {
        writeln!(out, "{usage}\n")?;
    }
    let rendered = error.to_string();
    let text = rendered
        .strip_prefix("error: ")
        .unwrap_or(&rendered)
        .trim_end();
    Output::new(mode)
        .status(out, "error", text)
        .map_err(std::io::Error::other)
}

/// Run a command with its deadline and interruption source, retaining uncertain writes.
pub async fn with_deadline(
    operation: impl std::future::Future<Output = Result<()>>,
    progress: &CommitProgress,
    deadline: Duration,
    interrupt: impl std::future::Future<Output = std::io::Result<()>>,
) -> Result<()> {
    tokio::select! {
        result = operation => result,
        _ = tokio::time::sleep(deadline) => Err(Error::Interrupted(Box::new(after_commit(progress.get(), Error::Deadline)))),
        result = interrupt => Err(Error::Interrupted(Box::new(after_commit(progress.get(), result.err().map_or(Error::Cancelled, Error::Io))))),
    }
}
fn timeout(m: &ArgMatches) -> u64 {
    match m.subcommand() {
        Some(("auth", auth)) if auth.subcommand_name() == Some("pair") => 20,
        Some(("auth", _)) | Some(("status", _)) => 15,
        Some(("doc", doc)) => match doc.subcommand_name() {
            Some("list" | "inspect") => 25,
            Some("render") => 45,
            Some("sync" | "archive" | "import" | "upload" | "upload-check" | "settings") => 300,
            _ => 30,
        },
        _ => 30,
    }
}
pub fn report(error: &Error, output: &Output, out: &mut dyn Write) -> Result<()> {
    let mut text = error
        .to_string()
        .trim_end_matches(char::is_whitespace)
        .to_string();
    if error.exit_status() == 4 && !matches!(error, Error::Credentials(_)) {
        text.push_str(": run 'remarkable auth pair <code>' with a new code from https://my.remarkable.com/device/desktop/connect");
    }
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy();
        text = text.replace(&format!("{home}/"), "~/");
    }
    output.status(out, "error", &text)?;
    fn ambiguous(error: &Error) -> Option<&remarkable_cloud::AmbiguousNameError> {
        match error {
            Error::Cloud(remarkable_cloud::Error::Ambiguous(error)) => Some(error),
            Error::Context(_, error) | Error::Commit(error) | Error::Interrupted(error) => {
                ambiguous(error)
            }
            Error::Multiple(errors) => errors.iter().find_map(ambiguous),
            _ => None,
        }
    }
    if let Some(error) = ambiguous(error) {
        let rows = error
            .candidates
            .iter()
            .map(|candidate| {
                vec![
                    candidate.item.id.clone(),
                    if candidate.unreachable {
                        if candidate.folder_path.is_empty() {
                            "-".into()
                        } else {
                            candidate.folder_path.clone()
                        }
                    } else {
                        format!("/{}", candidate.folder_path)
                    },
                    if candidate.unreachable { "no" } else { "yes" }.into(),
                ]
            })
            .collect::<Vec<_>>();
        output.table(out, &["ID", "FOLDER", "REACHABLE"], &rows)?;
    }
    Ok(())
}
pub async fn dispatch(
    m: &ArgMatches,
    options: &Options,
    output: &Output,
    out: &mut dyn Write,
    progress: &CommitProgress,
) -> Result<()> {
    match m.subcommand() {
        Some(("skill", skill)) => match skill.subcommand() {
            None => crate::skill::print(output, out, None, false),
            Some(("reference", reference)) => match reference.subcommand() {
                Some(("list", _)) => crate::skill::print(output, out, None, true),
                Some(("cat", cat)) => {
                    crate::skill::print(output, out, Some(s(cat, "topic")), false)
                }
                _ => Err(message("reference command required")),
            },
            _ => Err(message("unknown skill command")),
        },
        Some(("completion", completion)) => {
            let (shell, args) = completion
                .subcommand()
                .ok_or_else(|| message("shell required"))?;
            let shell = shell.parse::<clap_complete::Shell>().map_err(message)?;
            let mut root = commands::raw_command();
            if b(args, "no-descriptions") {
                fn remove(command: &mut clap::Command) {
                    *command = command.clone().about(None::<&'static str>);
                    for child in command.get_subcommands_mut() {
                        remove(child);
                    }
                    let ids = command
                        .get_arguments()
                        .map(|a| a.get_id().clone())
                        .collect::<Vec<_>>();
                    for id in ids {
                        *command = command
                            .clone()
                            .mut_arg(id, |a| a.help(None::<&'static str>));
                    }
                }
                remove(&mut root);
            }
            let mut bytes = Vec::new();
            clap_complete::generate(shell, &mut root, "remarkable", &mut bytes);
            out.write_all(&bytes)?;
            Ok(())
        }
        Some(("stroke", stroke)) => {
            let (verb, args) = stroke
                .subcommand()
                .ok_or_else(|| message("stroke command required"))?;
            let path = Path::new(s(args, "file.rm"));
            match verb {
                "inspect" => doc::stroke_inspect(path, output, out),
                "export" => doc::stroke_export(
                    path,
                    *args.get_one::<f64>("width").unwrap_or(&0.0),
                    *args.get_one::<f64>("height").unwrap_or(&0.0),
                    s(args, "output"),
                    output,
                    out,
                ),
                _ => Err(message("unknown stroke command")),
            }
        }
        Some(("auth", auth)) => {
            auth_command(
                auth.subcommand_name().unwrap_or("status"),
                auth.subcommand().map_or(auth, |(_, a)| a),
                options,
                output,
                out,
            )
            .await
        }
        Some(("status", args)) => auth_command("status", args, options, output, out).await,
        Some(("doc", doc)) => {
            let (verb, args) = doc
                .subcommand()
                .ok_or_else(|| message("document command required"))?;
            let mapping = if verb == "import" {
                Some(import::mapping(Path::new(s(args, "mapping")))?)
            } else {
                None
            };
            let settings_args = if verb == "settings" {
                Some(
                    args.subcommand()
                        .ok_or_else(|| message("settings command required"))?
                        .1,
                )
            } else {
                None
            };
            let settings_map = if let Some(args) = settings_args {
                Some(settings::mapping(Path::new(s(args, "mapping")))?)
            } else {
                None
            };
            let mut evidence = if verb == "upload-check" {
                Some(upload::read_evidence(Path::new(s(args, "evidence")))?)
            } else {
                None
            };
            let client = options.client(false)?;
            let query = s(args, "id-or-name");
            match verb {
                "list" => {
                    let items = doc::list(
                        &client,
                        s(args, "folder"),
                        s(args, "type"),
                        s(args, "query"),
                        *args.get_one::<i64>("limit").unwrap_or(&0),
                    )
                    .await?;
                    output.table(
                        out,
                        &["ID", "NAME", "TYPE", "MODIFIED"],
                        &items
                            .into_iter()
                            .map(|i| {
                                vec![
                                    i.id,
                                    i.metadata.visible_name,
                                    i.metadata.item_type,
                                    i.metadata.last_modified,
                                ]
                            })
                            .collect::<Vec<_>>(),
                    )?;
                    Ok(())
                }
                "tree" => {
                    let items = doc::list(&client, "", "", "", 0).await?;
                    output.tree(out, &doc::tree(&items))?;
                    Ok(())
                }
                "inspect" => doc::inspect(&client, query, b(args, "pages"), output, out).await,
                "cat" => doc::cat(&client, query, page(args), s(args, "format"), out).await,
                "search" => {
                    doc::search(
                        &client,
                        query,
                        s(args, "query"),
                        b(args, "word"),
                        output,
                        out,
                    )
                    .await
                }
                "links" => doc::links(&client, query, page(args), output, out).await,
                "render" => {
                    let path = Path::new(s(args, "output"));
                    doc::render(&client, query, page(args), dpi(args), path).await?;
                    output.status(
                        out,
                        "ok",
                        &format!("Rendered page {} to {}", page(args), output.path(path)),
                    )?;
                    Ok(())
                }
                "sync" => {
                    doc::sync(
                        &client,
                        query,
                        doc::SyncOptions {
                            output_dir: Path::new(s(args, "output-dir")),
                            format: s(args, "format"),
                            dpi: dpi(args),
                            force: b(args, "force"),
                        },
                        output,
                        out,
                    )
                    .await
                }
                "archive" => {
                    archive::archive(&client, query, Path::new(s(args, "output")), output, out)
                        .await
                }
                "import" => {
                    import::import(
                        &client,
                        s(args, "destination-uuid"),
                        mapping.as_deref().unwrap_or(&[]),
                        output,
                        out,
                        progress,
                    )
                    .await
                }
                "upload" => {
                    upload::upload(
                        &client,
                        Path::new(s(args, "pdf")),
                        upload::Options {
                            title: s(args, "title"),
                            folder: s(args, "folder"),
                            evidence: Path::new(s(args, "evidence")),
                            initialize_pages: b(args, "initialize-pages"),
                        },
                        output,
                        out,
                        progress,
                    )
                    .await
                }
                "upload-check" => {
                    let evidence = evidence
                        .as_mut()
                        .ok_or_else(|| message("missing upload evidence"))?;
                    upload::check(&client, evidence).await?;
                    upload::print(evidence, Path::new(s(args, "evidence")), output, out)
                }
                "settings" => {
                    let args = settings_args.ok_or_else(|| message("settings command required"))?;
                    settings::transfer(
                        &client,
                        settings::TransferOptions {
                            source: s(args, "source-uuid"),
                            destination: s(args, "destination-uuid"),
                            mapping: settings_map.as_deref().unwrap_or(&[]),
                            replace_viewport: b(args, "replace-viewport"),
                        },
                        output,
                        out,
                        progress,
                    )
                    .await
                }
                _ => Err(message("unknown document command")),
            }
        }
        _ => Err(message("command required")),
    }
}
async fn auth_command(
    verb: &str,
    args: &ArgMatches,
    options: &Options,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    let client = options.client(verb == "pair")?;
    match verb {
        "pair" => {
            let result = client.pair_device(s(args, "code")).await;
            let cfg = client.config().await;
            let path = crate::config::config_path(&options.config);
            if !cfg.device_token.is_empty() {
                cfg.write_file(&path)?;
            }
            result?;
            output.status(
                out,
                "ok",
                &format!(
                    "Device successfully paired. Credentials saved to {}",
                    output.path(&path)
                ),
            )?;
        }
        "token" => {
            writeln!(out, "{}", client.renew_token().await?)?;
        }
        _ => {
            let start = Instant::now();
            let root = client.get_root_state().await?;
            let latency = start.elapsed();
            let items = client.list_items(Default::default()).await?;
            if output.mode == Mode::Agent {
                output.key_values(
                    out,
                    &[
                        ("status", "connected".into()),
                        ("generation", root.generation.to_string()),
                        ("items", items.len().to_string()),
                        ("latency_ms", latency.as_millis().to_string()),
                    ],
                )?;
            } else {
                output.status(out, "ok", "Connected to reMarkable Cloud Sync v3")?;
                output.separator(out)?;
                output.key_values(
                    out,
                    &[
                        ("Status", "connected".into()),
                        ("Generation", root.generation.to_string()),
                        (
                            "Total Items",
                            format!("{} documents & collections", items.len()),
                        ),
                        ("API Latency", format!("{}ms", latency.as_millis())),
                        (
                            "Config Path",
                            output.path(&crate::config::config_path(&options.config)),
                        ),
                    ],
                )?;
            }
        }
    }
    Ok(())
}
