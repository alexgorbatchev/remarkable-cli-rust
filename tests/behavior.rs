mod common;
use cli_command_kit::Mode;
use common::*;
use remarkable_cli::{
    archive, doc, import, json,
    output::Output,
    runtime::{self, CommitProgress},
    settings::{self, PageMap, TransferOptions},
    snapshot, upload,
};
use remarkable_cloud::UpdateState;
use serde_json::{Value, json as value};
use std::{io::Read, path::Path, process::Command};

fn tempdir() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

#[cfg(unix)]
#[test]
fn interrupt_reports_the_pending_root_state_before_the_single_error() {
    use std::time::{Duration, Instant};
    let cloud = Cloud::seeded();
    cloud.state.lock().unwrap().root_delay = Duration::from_millis(700);
    let dir = tempdir();
    let config = dir.path().join("config");
    cloud.config(&config);
    let source = dir.path().join("source.rm");
    std::fs::write(&source, RM).unwrap();
    let mapping = dir.path().join("mapping.json");
    std::fs::write(
        &mapping,
        serde_json::to_vec(&value!([{"source":source,"page":0}])).unwrap(),
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_remarkable"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--no-cache",
            "doc",
            "import",
            DEST,
            "--mapping",
            mapping.to_str().unwrap(),
        ])
        .env("REMARKABLE_HOST", &cloud.url)
        .env("AGENT", "1")
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while cloud.state.lock().unwrap().commits() != 1 {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "root commit must start"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    // The child has installed Tokio's SIGINT handler before starting dispatch.
    assert_eq!(
        unsafe { libc::kill(i32::try_from(child.id()).unwrap(), libc::SIGINT) },
        0
    );
    let result = child.wait_with_output().unwrap();
    assert_eq!(
        result.status.code(),
        Some(6),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("state: commit-unknown"));
    let error = String::from_utf8_lossy(&result.stderr);
    assert_eq!(error.matches("ERR:").count(), 1, "{error}");
    assert!(error.contains("command cancelled"));
    assert_eq!(cloud.state.lock().unwrap().commits(), 1);
}

fn command(cloud: &Cloud, dir: &Path, args: &[&str], agent: bool) -> std::process::Output {
    let config = dir.join("config.json");
    cloud.config(&config);
    let mut command = Command::new(env!("CARGO_BIN_EXE_remarkable"));
    command
        .args(["--config", config.to_str().unwrap(), "--no-cache"])
        .args(args)
        .env("REMARKABLE_HOST", &cloud.url)
        .env("AGENT", if agent { "1" } else { "0" })
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env("XDG_CACHE_HOME", dir)
        .env(
            "PDFIUM_LIB_PATH",
            pdfium_bundled::ensure_pdfium_library(None).unwrap(),
        )
        .env("PDFIUM_NO_AUTO_DOWNLOAD", "1");
    command.output().unwrap()
}
fn text(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "status {:?}, stdout {}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[tokio::test]
async fn browse_cat_search_links_render_and_sync_use_native_data() {
    let cloud = Cloud::seeded();
    let client = cloud.client();
    let output = Output::new(Mode::Agent);
    let dir = tempdir();
    let items = doc::list(&client, "", "", "", 0).await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(
        doc::list(&client, FOLDER, "DocumentType", "destination", 1)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        doc::list(&client, "", "CollectionType", "missing", 0)
            .await
            .unwrap()
            .is_empty()
    );
    let tree = doc::tree(&items);
    assert_eq!(tree.label, "/");
    assert_eq!(tree.children.len(), 2);
    for query in [SOURCE, FOLDER] {
        let mut bytes = Vec::new();
        doc::inspect(&client, query, true, &output, &mut bytes)
            .await
            .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("Name:"));
        if query == SOURCE {
            assert!(text.contains("present (258 bytes)"));
            assert!(text.contains("empty"));
        }
    }
    for (format, expected) in [("pdf", PDF), ("rm", RM)] {
        let mut bytes = Vec::new();
        doc::cat(&client, SOURCE, 0, format, &mut bytes)
            .await
            .unwrap();
        assert_eq!(bytes, expected);
    }
    for format in ["text", "svg"] {
        let mut bytes = Vec::new();
        doc::cat(&client, SOURCE, 0, format, &mut bytes)
            .await
            .unwrap();
        if format == "text" {
            assert_eq!(
                bytes,
                remarkable_render::Document::from_bytes(PDF)
                    .unwrap()
                    .text(0)
                    .unwrap()
                    .as_bytes()
            );
        } else {
            assert!(!bytes.is_empty());
        }
    }
    assert_eq!(
        doc::cat(&client, SOURCE, 99, "rm", &mut Vec::new())
            .await
            .unwrap_err()
            .exit_status(),
        3
    );
    assert!(
        doc::cat(&client, SOURCE, 1, "rm", &mut Vec::new())
            .await
            .is_err()
    );
    for mode in [Mode::Agent, Mode::Human] {
        let output = Output::new(mode);
        for query in ["Link", "not-in-document"] {
            doc::search(&client, SOURCE, query, false, &output, &mut Vec::new())
                .await
                .unwrap();
        }
        let mut bytes = Vec::new();
        doc::links(&client, SOURCE, 0, &output, &mut bytes)
            .await
            .unwrap();
        assert!(!bytes.is_empty());
    }
    let png = dir.path().join("nested/page.png");
    doc::render(&client, SOURCE, 0, 24, &png).await.unwrap();
    assert!(std::fs::read(png).unwrap().starts_with(b"\x89PNG"));
    assert_eq!(
        doc::render(&client, SOURCE, 99, 72, &dir.path().join("bad.png"))
            .await
            .unwrap_err()
            .exit_status(),
        3
    );
    for (format, mode) in [
        ("rm", Mode::Agent),
        ("svg", Mode::Human),
        ("png", Mode::Agent),
    ] {
        let options = || doc::SyncOptions {
            output_dir: dir.path(),
            format,
            dpi: 24,
            force: false,
        };
        let mut bytes = Vec::new();
        doc::sync(&client, SOURCE, options(), &Output::new(mode), &mut bytes)
            .await
            .unwrap();
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains(if mode == Mode::Agent {
                    "written:"
                } else {
                    "Sync complete"
                })
        );
        let mut bytes = Vec::new();
        doc::sync(&client, SOURCE, options(), &Output::new(mode), &mut bytes)
            .await
            .unwrap();
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains(if mode == Mode::Agent {
                    "skipped:"
                } else {
                    "Skipped existing"
                })
        );
        doc::sync(
            &client,
            SOURCE,
            doc::SyncOptions {
                force: true,
                ..options()
            },
            &output,
            &mut Vec::new(),
        )
        .await
        .unwrap();
    }
    assert_eq!(cloud.state.lock().unwrap().writes(), 0);
}

#[tokio::test]
async fn sync_sanitizes_dot_titles_instead_of_escaping_destination() {
    let cloud = Cloud::new();
    cloud
        .state
        .lock()
        .unwrap()
        .document(SOURCE, "..", "", content(&["p0"]), true);
    let dir = tempdir();
    doc::sync(
        &cloud.client(),
        SOURCE,
        doc::SyncOptions {
            output_dir: dir.path(),
            format: "rm",
            dpi: 72,
            force: false,
        },
        &Output::new(Mode::Agent),
        &mut Vec::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(dir.path().join(SOURCE).join("page-000.rm")).unwrap(),
        RM
    );
    assert!(!dir.path().parent().unwrap().join("page-000.rm").exists());
}

#[tokio::test]
async fn archives_preserve_all_native_files_and_never_overwrite() {
    let cloud = Cloud::seeded();
    let dir = tempdir();
    let path = dir.path().join("document.zip");
    let mut bytes = Vec::new();
    archive::archive(
        &cloud.client(),
        SOURCE,
        &path,
        &Output::new(Mode::Agent),
        &mut bytes,
    )
    .await
    .unwrap();
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("state: verified")
    );
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let files = cloud.state.lock().unwrap().files(SOURCE);
    for (name, data) in files {
        let mut bytes = Vec::new();
        zip.by_name(&format!("files/{name}"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, data);
    }
    let mut bytes = Vec::new();
    zip.by_name("evidence/snapshot.json")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["document_id"],
        SOURCE
    );
    let old = std::fs::read(&path).unwrap();
    assert!(
        archive::archive(
            &cloud.client(),
            SOURCE,
            &path,
            &Output::new(Mode::Agent),
            &mut Vec::new()
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), old);
    assert!(
        archive::archive(
            &cloud.client(),
            FOLDER,
            &dir.path().join("folder.zip"),
            &Output::new(Mode::Agent),
            &mut Vec::new()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn import_validates_entire_mapping_before_writes_and_preserves_other_files() {
    let cloud = Cloud::seeded();
    let client = cloud.client();
    let dir = tempdir();
    let rm = dir.path().join("source.rm");
    std::fs::write(&rm, RM).unwrap();
    let progress = CommitProgress::default();
    let output = Output::new(Mode::Agent);
    let before = cloud.state.lock().unwrap().files(DEST);
    let rows = [import::PageImport {
        source: rm.clone(),
        page: 0,
    }];
    let mut bytes = Vec::new();
    import::import(&client, DEST, &rows, &output, &mut bytes, &progress)
        .await
        .unwrap();
    assert_eq!(progress.get(), UpdateState::Verified);
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("state: verified")
    );
    let after = cloud.state.lock().unwrap().files(DEST);
    assert_eq!(after[&format!("{DEST}/d0.rm")], RM);
    for suffix in ["pdf", "content", "pagedata"] {
        assert_eq!(
            before[&format!("{DEST}.{suffix}")],
            after[&format!("{DEST}.{suffix}")]
        );
    }
    let meta = json::object(&after[&format!("{DEST}.metadata")]).unwrap();
    assert_eq!(
        json::value(&meta, "unknown").unwrap().to_string(),
        "9007199254740993"
    );
    assert!(
        import::import(
            &client,
            DEST,
            &rows,
            &output,
            &mut Vec::new(),
            &CommitProgress::default()
        )
        .await
        .is_err()
    );
    let writes = cloud.state.lock().unwrap().writes();
    for rows in [
        vec![
            import::PageImport {
                source: rm.clone(),
                page: 1,
            },
            import::PageImport {
                source: rm.clone(),
                page: 999,
            },
        ],
        vec![
            import::PageImport {
                source: rm.clone(),
                page: 1,
            },
            import::PageImport {
                source: rm.clone(),
                page: 1,
            },
        ],
        vec![import::PageImport {
            source: dir.path().join("missing.rm"),
            page: 1,
        }],
    ] {
        assert!(
            import::import(
                &client,
                DEST,
                &rows,
                &output,
                &mut Vec::new(),
                &CommitProgress::default()
            )
            .await
            .is_err()
        );
        assert_eq!(cloud.state.lock().unwrap().writes(), writes);
    }
    assert!(
        import::import(
            &client,
            FOLDER,
            &rows,
            &output,
            &mut Vec::new(),
            &CommitProgress::default()
        )
        .await
        .is_err()
    );
    assert!(import::empty_native(RM).is_err());
    import::empty_native(rmscene::HEADER_V6).unwrap();
}

#[tokio::test]
async fn settings_transfer_verifies_source_and_destination_and_avoids_noop_commit() {
    let cloud = Cloud::seeded();
    let client = cloud.client();
    let map = [
        PageMap {
            source_page: 0,
            destination_page: 0,
        },
        PageMap {
            source_page: 1,
            destination_page: 1,
        },
    ];
    for (source, destination) in [(SOURCE, SOURCE), ("invalid", DEST), (SOURCE, "invalid")] {
        assert!(
            settings::transfer(
                &client,
                TransferOptions {
                    source,
                    destination,
                    mapping: &map,
                    replace_viewport: false
                },
                &Output::new(Mode::Agent),
                &mut Vec::new(),
                &CommitProgress::default()
            )
            .await
            .is_err()
        );
        assert_eq!(cloud.state.lock().unwrap().requests.len(), 0);
    }
    let opts = || TransferOptions {
        source: SOURCE,
        destination: DEST,
        mapping: &map,
        replace_viewport: false,
    };
    let progress = CommitProgress::default();
    let before = cloud.state.lock().unwrap().files(SOURCE);
    settings::transfer(
        &client,
        opts(),
        &Output::new(Mode::Agent),
        &mut Vec::new(),
        &progress,
    )
    .await
    .unwrap();
    assert_eq!(progress.get(), UpdateState::Verified);
    assert_eq!(cloud.state.lock().unwrap().commits(), 0);
    {
        let mut state = cloud.state.lock().unwrap();
        let mut incoming = content(&["p0", "p1"]);
        incoming["tags"] = value!([{"name":"new","timestamp":9007199254740993u64}]);
        incoming["pageTags"] = value!([{"name":"marked","timestamp":1,"pageId":"p1"}]);
        state.document(SOURCE, "Source", "", incoming, true);
    }
    let mut bytes = Vec::new();
    settings::transfer(
        &client,
        opts(),
        &Output::new(Mode::Agent),
        &mut bytes,
        &progress,
    )
    .await
    .unwrap();
    assert_eq!(cloud.state.lock().unwrap().commits(), 1);
    let state = cloud.state.lock().unwrap();
    assert_eq!(
        state.files(SOURCE)[&format!("{SOURCE}.pdf")],
        before[&format!("{SOURCE}.pdf")]
    );
    let dst = json::object(&state.files(DEST)[&format!("{DEST}.content")]).unwrap();
    assert_eq!(json::value(&dst, "pageTags").unwrap()[0]["pageId"], "d1");
    assert_eq!(
        json::value(&dst, "unknown").unwrap()["large"].to_string(),
        "9007199254740993"
    );
}

#[tokio::test]
async fn write_failures_retain_unknown_and_committed_progress() {
    for (status, corrupt, expected) in [
        (409, false, UpdateState::Staged),
        (500, false, UpdateState::CommitUnknown),
        (200, true, UpdateState::Committed),
    ] {
        let cloud = Cloud::seeded();
        {
            let mut state = cloud.state.lock().unwrap();
            state.commit_status = status;
            state.corrupt_after_commit = corrupt;
        }
        let dir = tempdir();
        let rm = dir.path().join("source.rm");
        std::fs::write(&rm, RM).unwrap();
        let progress = CommitProgress::default();
        let error = import::import(
            &cloud.client(),
            DEST,
            &[import::PageImport {
                source: rm,
                page: 0,
            }],
            &Output::new(Mode::Agent),
            &mut Vec::new(),
            &progress,
        )
        .await
        .unwrap_err();
        assert_eq!(progress.get(), expected);
        assert_eq!(
            error.exit_status(),
            if expected == UpdateState::Staged {
                1
            } else {
                6
            }
        );
        assert_eq!(cloud.state.lock().unwrap().commits(), 1);
    }
}

#[tokio::test]
async fn upload_records_recoverable_evidence_and_preserves_native_page_identity() {
    for initialized in [false, true] {
        let cloud = Cloud::seeded();
        let dir = tempdir();
        let pdf = dir.path().join("input.pdf");
        std::fs::write(&pdf, PDF).unwrap();
        let evidence = dir.path().join("creation.json");
        let progress = CommitProgress::default();
        let mut bytes = Vec::new();
        upload::upload(
            &cloud.client(),
            &pdf,
            upload::Options {
                title: "Uploaded",
                folder: FOLDER,
                evidence: &evidence,
                initialize_pages: initialized,
            },
            &Output::new(Mode::Agent),
            &mut bytes,
            &progress,
        )
        .await
        .unwrap();
        assert_eq!(progress.get(), UpdateState::Verified);
        let mut e = upload::read_evidence(&evidence).unwrap();
        assert_eq!(e.result.state, UpdateState::Verified);
        assert_eq!(
            e.native_pages,
            if initialized {
                upload::INITIALIZED
            } else {
                upload::PENDING
            }
        );
        assert!(String::from_utf8(bytes).unwrap().contains("commit-unknown"));
        let files = cloud.state.lock().unwrap().files(&e.result.id);
        assert_eq!(files[&format!("{}.pdf", e.result.id)], PDF);
        if initialized {
            upload::check_identity(&e, &files[&format!("{}.content", e.result.id)]).unwrap();
        }
        upload::check(&cloud.client(), &mut e).await.unwrap();
        assert_eq!(cloud.state.lock().unwrap().commits(), 1);
        assert!(upload::write_evidence(&evidence, &e, true).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&evidence).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut invalid = e.clone();
        invalid.files[0].sha256 = "bad".into();
        assert!(upload::validate_evidence(&invalid).is_err());
        invalid = e.clone();
        invalid.files[0] = invalid.files[1].clone();
        assert!(upload::validate_evidence(&invalid).is_err());
        invalid = e.clone();
        invalid.native_pages = "unknown".into();
        assert!(upload::validate_evidence(&invalid).is_err());
        invalid = e.clone();
        invalid.version = 999;
        assert!(upload::validate_evidence(&invalid).is_err());
        if initialized {
            invalid = e.clone();
            invalid.page_ids[0] = invalid.result.id.clone();
            assert!(upload::validate_evidence(&invalid).is_err());
            invalid = e.clone();
            invalid.page_ids.pop();
            assert!(upload::validate_evidence(&invalid).is_err());
        }
    }
}

#[tokio::test]
async fn upload_preflight_rejects_collisions_missing_and_invalid_folders() {
    let cloud = Cloud::seeded();
    let client = cloud.client();
    assert!(upload::preflight(&client, "Source", "").await.is_err());
    assert_eq!(
        upload::preflight(&client, "New", "missing")
            .await
            .unwrap_err()
            .exit_status(),
        3
    );
    assert!(upload::preflight(&client, "New", SOURCE).await.is_err());
    cloud
        .state
        .lock()
        .unwrap()
        .collection(FOLDER, "Folder", FOLDER);
    assert!(upload::preflight(&client, "New", FOLDER).await.is_err());
    cloud
        .state
        .lock()
        .unwrap()
        .collection(FOLDER, "Folder", "trash");
    assert!(upload::preflight(&client, "New", FOLDER).await.is_err());
}

#[tokio::test]
async fn fresh_snapshots_reject_corruption_and_changed_associations() {
    let cloud = Cloud::seeded();
    let client = cloud.client();
    let (root, manifest) = snapshot::root(&client).await.unwrap();
    let before = snapshot::read(&client, &manifest, SOURCE).await.unwrap();
    snapshot::verify(&client, &manifest, &before, None)
        .await
        .unwrap();
    assert_eq!(
        snapshot::read(&client, &manifest, "missing")
            .await
            .unwrap_err()
            .exit_status(),
        3
    );
    cloud
        .state
        .lock()
        .unwrap()
        .document(SOURCE, "Changed", "", content(&["p0"]), true);
    let (_, manifest) = snapshot::root(&client).await.unwrap();
    assert!(
        snapshot::verify(&client, &manifest, &before, None)
            .await
            .is_err()
    );
    assert!(
        snapshot::verify(&client, &manifest, &before, Some(b"{}"))
            .await
            .is_err()
    );
    assert!(snapshot::guard(&client, &root).await.is_err());
    let hash = cloud.state.lock().unwrap().root.hash.clone();
    cloud
        .state
        .lock()
        .unwrap()
        .blobs
        .insert(hash, b"corrupt".to_vec());
    assert!(snapshot::root(&client).await.is_err());
}

#[test]
fn subprocess_cli_executes_all_read_subjects_and_completion_shells() {
    let cloud = Cloud::seeded();
    let dir = tempdir();
    for args in [
        vec!["doc", "list"],
        vec!["doc", "tree"],
        vec!["doc", "inspect", SOURCE, "--pages"],
        vec!["doc", "search", SOURCE, "Link"],
        vec!["doc", "links", SOURCE],
        vec!["auth", "status"],
        vec!["status"],
        vec!["auth", "token"],
    ] {
        for agent in [false, true] {
            assert!(
                !text(&command(&cloud, dir.path(), &args, agent)).is_empty() || args[1] == "search"
            );
        }
    }
    for shell in ["bash", "fish", "zsh", "powershell"] {
        for descriptions in [false, true] {
            let mut args = vec!["completion", shell];
            if descriptions {
                args.push("--no-descriptions");
            }
            assert!(text(&command(&cloud, dir.path(), &args, true)).contains("remarkable"));
        }
    }
    let pdf = command(
        &cloud,
        dir.path(),
        &["doc", "cat", SOURCE, "--format=pdf"],
        true,
    );
    assert!(pdf.status.success());
    assert_eq!(pdf.stdout, PDF);
    let png = dir.path().join("page.png");
    text(&command(
        &cloud,
        dir.path(),
        &[
            "doc",
            "render",
            SOURCE,
            "--dpi=24",
            "--output",
            png.to_str().unwrap(),
        ],
        true,
    ));
    assert!(std::fs::read(png).unwrap().starts_with(b"\x89PNG"));
    text(&command(
        &cloud,
        dir.path(),
        &[
            "doc",
            "sync",
            SOURCE,
            "--format=rm",
            "--output-dir",
            dir.path().to_str().unwrap(),
        ],
        true,
    ));
    let archive = dir.path().join("doc.zip");
    text(&command(
        &cloud,
        dir.path(),
        &[
            "doc",
            "archive",
            SOURCE,
            "--output",
            archive.to_str().unwrap(),
        ],
        true,
    ));
}

#[test]
fn subprocess_stroke_and_embedded_skill_contracts_are_complete() {
    let cloud = Cloud::new();
    let dir = tempdir();
    let rm = dir.path().join("source.rm");
    std::fs::write(&rm, RM).unwrap();
    let rm = rm.to_str().unwrap();
    for agent in [false, true] {
        assert!(
            text(&command(
                &cloud,
                dir.path(),
                &["stroke", "inspect", rm],
                agent
            ))
            .contains("Lines:")
        );
        assert!(
            text(&command(
                &cloud,
                dir.path(),
                &["stroke", "export", rm, "--width=100", "--height=200"],
                agent
            ))
            .contains("<svg")
        );
        let svg = dir.path().join(format!("out-{agent}.svg"));
        text(&command(
            &cloud,
            dir.path(),
            &["stroke", "export", rm, "--output", svg.to_str().unwrap()],
            agent,
        ));
        assert!(std::fs::read_to_string(svg).unwrap().contains("<svg"));
    }
    assert_eq!(
        text(&command(&cloud, dir.path(), &["skill"], true)),
        remarkable_cli::skill::SKILL
    );
    let topics = remarkable_cli::skill::topics();
    assert!(
        text(&command(
            &cloud,
            dir.path(),
            &["skill", "reference", "list"],
            true
        ))
        .contains(topics[0])
    );
    for topic in topics {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("skills/remarkable/references")
                .join(format!("{topic}.md")),
        )
        .unwrap();
        assert_eq!(
            text(&command(
                &cloud,
                dir.path(),
                &["skill", "reference", "cat", topic],
                true
            )),
            expected
        );
    }
    assert_eq!(
        text(&command(&cloud, dir.path(), &["--version"], true)),
        "dev\n"
    );
    assert!(
        text(&command(&cloud, dir.path(), &[], true)).starts_with(remarkable_cli::commands::ALERT)
    );
}

#[test]
fn subprocess_parse_errors_show_usage_but_runtime_errors_print_once_without_usage() {
    let cloud = Cloud::seeded();
    let dir = tempdir();
    for args in [
        vec!["doc", "cat", SOURCE, "--page=-1"],
        vec!["doc", "settings", "transfer", SOURCE, SOURCE, "--mapping=x"],
        vec!["auth", "pair", "bad"],
        vec!["doc", "render", SOURCE, "--dpi=invalid", "--output=x"],
        vec!["doc", "inspect", SOURCE, "--pages=invalid"],
        vec!["skill", "reference", "cat", "missing"],
    ] {
        let result = command(&cloud, dir.path(), &args, true);
        assert_eq!(result.status.code(), Some(1));
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("Usage:"), "{error}");
        assert_eq!(error.matches("ERR:").count(), 1, "{error}");
        assert!(
            error.find("Usage:").unwrap() < error.find("ERR:").unwrap(),
            "{error}"
        );
        assert_eq!(cloud.state.lock().unwrap().requests.len(), 0);
    }
    let result = command(
        &cloud,
        dir.path(),
        &["stroke", "inspect", "missing.rm"],
        true,
    );
    assert_eq!(result.status.code(), Some(3));
    let error = String::from_utf8_lossy(&result.stderr);
    assert_eq!(error.matches("ERR:").count(), 1);
    assert!(!error.contains("Usage:"));
    for (status, exit) in [(401, 4), (404, 1), (500, 5)] {
        cloud.state.lock().unwrap().reject_status = status;
        let result = command(&cloud, dir.path(), &["status"], true);
        assert_eq!(
            result.status.code(),
            Some(exit),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[tokio::test]
async fn execute_library_reports_parse_help_and_credentials_without_network() {
    for args in [
        vec!["remarkable", "--help"],
        vec!["remarkable", "doc", "cat", "--help"],
        vec!["remarkable", "help", "doc", "cat"],
        vec!["remarkable", "doc"],
        vec!["remarkable", "auth"],
        vec!["remarkable", "doc", "settings"],
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            runtime::execute(args, Mode::Agent, &mut out, &mut err).await,
            0
        );
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with(remarkable_cli::commands::ALERT)
        );
        assert!(err.is_empty());
    }
    let dir = tempdir();
    let config = dir.path().join("missing.json");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        runtime::execute(
            ["remarkable", "--config", config.to_str().unwrap(), "status"],
            Mode::Agent,
            &mut out,
            &mut err
        )
        .await,
        4
    );
    assert!(String::from_utf8(err).unwrap().contains("auth pair"));
}

#[tokio::test]
async fn deadline_and_cancellation_keep_pending_root_commit_uncertain() {
    use std::time::Duration;
    for cancel in [false, true] {
        let cloud = Cloud::seeded();
        cloud.state.lock().unwrap().root_delay = Duration::from_millis(700);
        let dir = tempdir();
        let source = dir.path().join("source.rm");
        std::fs::write(&source, RM).unwrap();
        let rows = [import::PageImport { source, page: 0 }];
        let client = cloud.client();
        let progress = CommitProgress::default();
        let output = Output::new(Mode::Agent);
        let mut bytes = Vec::new();
        let interrupt = async {
            if cancel {
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if cloud.state.lock().unwrap().commits() == 1 {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                })
                .await
                .expect("root PUT must start");
                Ok(())
            } else {
                std::future::pending::<std::io::Result<()>>().await
            }
        };
        let error = runtime::with_deadline(
            import::import(&client, DEST, &rows, &output, &mut bytes, &progress),
            &progress,
            Duration::from_millis(300),
            interrupt,
        )
        .await
        .unwrap_err();
        assert_eq!(progress.get(), UpdateState::CommitUnknown);
        assert_eq!(error.exit_status(), 6, "{error}");
        assert_eq!(cloud.state.lock().unwrap().commits(), 1);
    }
    let progress = CommitProgress::default();
    let pending = || std::future::pending::<remarkable_cli::error::Result<()>>();
    assert_eq!(
        runtime::with_deadline(pending(), &progress, Duration::ZERO, std::future::pending())
            .await
            .unwrap_err()
            .exit_status(),
        5
    );
    assert_eq!(
        runtime::with_deadline(pending(), &progress, Duration::from_secs(1), async {
            Ok(())
        })
        .await
        .unwrap_err()
        .exit_status(),
        1
    );
    assert_eq!(
        runtime::with_deadline(pending(), &progress, Duration::from_secs(1), async {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .await
        .unwrap_err()
        .exit_status(),
        1
    );
}

#[test]
fn subprocess_native_write_commands_create_recovery_evidence_and_transfer_settings() {
    let cloud = Cloud::seeded();
    let dir = tempdir();
    text(&command(
        &cloud,
        dir.path(),
        &["auth", "pair", "12345678"],
        true,
    ));
    let config = remarkable_cloud::Config::read_file(&dir.path().join("config.json")).unwrap();
    assert_eq!(config.device_token, "fake-device");
    let pdf = dir.path().join("input.pdf");
    let evidence = dir.path().join("created.json");
    std::fs::write(&pdf, PDF).unwrap();
    text(&command(
        &cloud,
        dir.path(),
        &[
            "doc",
            "upload",
            pdf.to_str().unwrap(),
            "--title=Uploaded",
            "--initialize-pages",
            "--evidence",
            evidence.to_str().unwrap(),
        ],
        true,
    ));
    let uploaded = upload::read_evidence(&evidence).unwrap();
    assert_eq!(uploaded.result.state, UpdateState::Verified);
    assert!(
        text(&command(
            &cloud,
            dir.path(),
            &["doc", "upload-check", evidence.to_str().unwrap()],
            true
        ))
        .contains("state: verified")
    );
    let rm = dir.path().join("input.rm");
    std::fs::write(&rm, RM).unwrap();
    let mapping = dir.path().join("mapping.json");
    std::fs::write(
        &mapping,
        serde_json::to_vec(&value!([{"source":rm,"page":0}])).unwrap(),
    )
    .unwrap();
    assert!(
        text(&command(
            &cloud,
            dir.path(),
            &[
                "doc",
                "import",
                DEST,
                "--mapping",
                mapping.to_str().unwrap()
            ],
            true
        ))
        .contains("state: verified")
    );
    let mut incoming = content(&["p0", "p1"]);
    incoming["tags"] = value!([{"name":"transfer","timestamp":1}]);
    cloud
        .state
        .lock()
        .unwrap()
        .document(SOURCE, "Source", "", incoming, true);
    std::fs::write(&mapping, br#"[{"source_page":0,"destination_page":0}]"#).unwrap();
    assert!(
        text(&command(
            &cloud,
            dir.path(),
            &[
                "doc",
                "settings",
                "transfer",
                SOURCE,
                DEST,
                "--mapping",
                mapping.to_str().unwrap()
            ],
            false
        ))
        .contains("verified")
    );
    let files = cloud.state.lock().unwrap().files(DEST);
    let content: Value = serde_json::from_slice(&files[&format!("{DEST}.content")]).unwrap();
    assert_eq!(content["tags"][0]["name"], "transfer");
    cloud.state.lock().unwrap().commit_status = 500;
    let failed = dir.path().join("uncertain.json");
    let result = command(
        &cloud,
        dir.path(),
        &[
            "doc",
            "upload",
            pdf.to_str().unwrap(),
            "--title=Uncertain",
            "--evidence",
            failed.to_str().unwrap(),
        ],
        true,
    );
    assert_eq!(result.status.code(), Some(6));
    assert_eq!(
        upload::read_evidence(&failed).unwrap().result.state,
        UpdateState::CommitUnknown
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stderr)
            .matches("ERR:")
            .count(),
        1
    );
}

fn run(cloud: &Cloud, home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_remarkable"));
    command
        .env("HOME", home)
        .env("AGENT", "1")
        .env("REMARKABLE_HOST", &cloud.url)
        .env_remove("REMARKABLE_CONFIG")
        .env_remove("REMARKABLE_CACHE_DIR")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME");
    command
}
fn credentials(path: &Path, user: &str) {
    remarkable_cloud::Config {
        device_token: "fake-device".into(),
        user_token: user.into(),
    }
    .write_file(path)
    .unwrap();
}
fn connected(cloud: &Cloud, mut command: Command, user: &str) {
    let result = command.arg("status").output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let state = cloud.state.lock().unwrap();
    let root = state
        .requests
        .iter()
        .rev()
        .find(|r| r.path == "/sync/v3/root")
        .unwrap();
    assert_eq!(root.headers["authorization"], format!("Bearer {user}"));
}

#[test]
fn isolated_processes_resolve_config_and_cache_precedence_without_global_environment_races() {
    let cloud = Cloud::seeded();
    let dir = tempdir();
    let home = dir.path();
    credentials(&home.join(".rmapi"), "fallback-user");
    connected(&cloud, run(&cloud, home), "fallback-user");
    assert!(home.join(".cache/remarkable-cli/blobs").exists());
    let xdg = home.join("config-base");
    credentials(&xdg.join("remarkable-cli/config.json"), "xdg-user");
    let cache = home.join("cache-base");
    let mut command = run(&cloud, home);
    command
        .env("XDG_CONFIG_HOME", &xdg)
        .env("XDG_CACHE_HOME", &cache);
    connected(&cloud, command, "xdg-user");
    assert!(cache.join("remarkable-cli/blobs").exists());
    let env_config = home.join("env.json");
    credentials(&env_config, "environment-user");
    let env_cache = home.join("environment-cache");
    let mut command = run(&cloud, home);
    command
        .env("XDG_CONFIG_HOME", &xdg)
        .env("REMARKABLE_CONFIG", &env_config)
        .env("REMARKABLE_CACHE_DIR", &env_cache);
    connected(&cloud, command, "environment-user");
    assert!(env_cache.join("blobs").exists());
    let explicit = home.join("explicit.json");
    credentials(&explicit, "explicit-user");
    let explicit_cache = home.join("explicit-cache");
    let mut command = run(&cloud, home);
    command
        .env("REMARKABLE_CONFIG", &env_config)
        .env("REMARKABLE_CACHE_DIR", &env_cache)
        .args([
            "--config",
            explicit.to_str().unwrap(),
            "--cache-dir",
            explicit_cache.to_str().unwrap(),
        ]);
    connected(&cloud, command, "explicit-user");
    assert!(explicit_cache.join("blobs").exists());
    let default_xdg = home.join(".config/remarkable-cli/config.json");
    credentials(&default_xdg, "default-xdg-user");
    let mut command = run(&cloud, home);
    command
        .arg("--no-cache")
        .env("REMARKABLE_CACHE_DIR", home.join("disabled-cache"));
    connected(&cloud, command, "default-xdg-user");
    assert!(!home.join("disabled-cache").exists());
}

#[test]
fn pairing_creates_fallback_credentials_and_config_failures_print_once() {
    let cloud = Cloud::new();
    let dir = tempdir();
    let result = run(&cloud, dir.path())
        .args(["auth", "pair", "12345678"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        remarkable_cloud::Config::read_file(&dir.path().join(".rmapi"))
            .unwrap()
            .device_token,
        "fake-device"
    );
    for (name, data) in [
        ("invalid.json", b"not credentials".as_slice()),
        ("empty.json", b""),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, data).unwrap();
        let result = run(&cloud, dir.path())
            .args(["--config", path.to_str().unwrap(), "status"])
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(4),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stderr)
                .matches("ERR:")
                .count(),
            1
        );
    }
    let result = run(&cloud, dir.path())
        .args(["--config", dir.path().to_str().unwrap(), "status"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let result = run(&cloud, dir.path())
        .env("REMARKABLE_HOST", ":bad address")
        .arg("status")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
}
