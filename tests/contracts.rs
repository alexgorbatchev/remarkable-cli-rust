use cli_command_kit::Mode;
use remarkable_cli::{
    commands,
    error::{Error, after_commit},
    json,
    output::{Output, TreeNode},
    search,
    settings::{self, PageMap},
    upload, validation as v,
};
use remarkable_cloud::UpdateState;

#[test]
fn native_upload_files_reject_inconsistent_identity_instead_of_panicking() {
    let id = "11111111-1111-4111-8111-111111111111";
    let page = "22222222-2222-4222-8222-222222222222";
    let mut evidence = upload::Evidence {
        version: 1,
        title: "Test".into(),
        folder: String::new(),
        pages: 1,
        native_pages: upload::INITIALIZED.into(),
        page_ids: vec![page.into()],
        result: remarkable_cloud::CreateResult {
            id: id.into(),
            ..Default::default()
        },
        files: vec![],
    };
    for ids in [
        vec![page.into(), page.into()],
        vec![],
        vec![id.into()],
        vec!["invalid".into()],
    ] {
        evidence.page_ids = ids;
        assert!(upload::files(id, vec![], &evidence, page).is_err());
    }
    evidence.page_ids = vec![page.into()];
    let files = upload::files(id, vec![1], &evidence, page).unwrap();
    let fields = json::object(
        &files
            .iter()
            .find(|f| f.name.ends_with(".content"))
            .unwrap()
            .data,
    )
    .unwrap();
    assert_eq!(json::page_ids(&fields).unwrap(), [page]);
}

#[test]
fn index_syntax_preserves_signed_native_range_and_go_base_zero_rules() {
    for (input, value) in [
        ("0", 0),
        ("+12", 12),
        ("012", 10),
        ("0x_FF", 255),
        ("0b1_01", 5),
        ("0o_12", 10),
        ("0_12", 10),
    ] {
        assert_eq!(v::index(input), Ok(value), "{input}");
    }
    for input in [
        "_1",
        "1_",
        "1__2",
        "0x__1",
        "-1",
        "--1",
        "+-1",
        "-+0",
        "08",
        "",
        "18446744073709551615",
    ] {
        assert!(v::index(input).is_err(), "{input}");
    }
}

#[test]
fn strict_json_and_native_page_identity_reject_ambiguous_inputs() {
    for input in [
        br#"{"pages":["a"],"pageCount":null}"#.as_slice(),
        br#"{"pages":["a"],"pageCount":1,"pageCount":2}"#,
        br#"{"pages":["a","a"]}"#,
        br#"{"pages":["../escape"]}"#,
        br#"{"pages":["a"],"cPages":{"pages":[{"id":"b"}]}}"#,
        br#"{"cPages":{"pages":[{"id":"a","deleted":{"value":1}}]}}"#,
        br#"{"pages":["a"],"pageCount":2}"#,
        br#"{"pages":[]}"#,
        br#"{"cPages":null}"#,
    ] {
        assert!(
            json::object(input)
                .and_then(|o| json::page_ids(&o))
                .is_err(),
            "{}",
            String::from_utf8_lossy(input)
        );
    }
    for input in [
        br#"{"pages":["a"],"pageCount":1}"#.as_slice(),
        br#"{"pages":["a"],"cPages":{"pages":[{"id":"a","deleted":{"value":0}}]}}"#,
        br#"{"cPages":{"pages":[{"id":"a"}]}}"#,
    ] {
        assert_eq!(
            json::page_ids(&json::object(input).unwrap()).unwrap(),
            ["a"]
        );
    }
    for input in [
        b"{} {}".as_slice(),
        b"\xff",
        br#"{"a":{"x":1,"x":2}}"#,
        b"[true, null, {\"x\": [1,1]}]",
    ] {
        let result = json::validate(input);
        if input.starts_with(b"[") {
            result.unwrap();
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn json_numeric_comparison_is_exact_without_expanding_exponents() {
    for (a, b, same) in [
        ("1", "1.00e0", true),
        ("-0", "0e99999999999999999999999999", true),
        ("9007199254740993", "9007199254740992", false),
        ("1e99999999999999999999", "10e99999999999999999998", true),
        ("{\"a\":1,\"b\":[2]}", "{\"b\":[2.0],\"a\":1e0}", true),
        ("[1,2]", "[2,1]", false),
    ] {
        let a = serde_json::value::RawValue::from_string(a.into()).unwrap();
        let b = serde_json::value::RawValue::from_string(b.into()).unwrap();
        assert_eq!(json::same(&a, &b), same);
    }
}

#[test]
fn mapped_tags_preserve_unknown_fields_and_viewport_requires_review() {
    let src=json::object(br#"{"pages":["s0","s1"],"zoomMode":"bestFit","customZoomScale":1,"tags":[{"name":"doc","timestamp":9007199254740993,"future":true}],"pageTags":[{"name":"page","timestamp":1e2,"pageId":"s1"}]}"#).unwrap();
    let dst=json::object(br#"{"pages":["d0","d1"],"zoomMode":"customFit","customZoomScale":2,"customZoomCenterX":3,"unknown":{"large":9007199254740993},"pageTags":[{"name":"keep","timestamp":1,"pageId":"d1"}]}"#).unwrap();
    let map = [PageMap {
        source_page: 1,
        destination_page: 0,
    }];
    let mut diffs = Vec::new();
    assert!(settings::merge(&src, &dst, &map, false, &mut diffs).is_err());
    assert_eq!(diffs.len(), 3);
    let merged =
        json::object(&settings::merge(&src, &dst, &map, true, &mut Vec::new()).unwrap()).unwrap();
    assert!(json::same(&dst["unknown"], &merged["unknown"]));
    assert!(!merged.contains_key("customZoomCenterX"));
    assert_eq!(json::value(&merged, "pageTags").unwrap()[1]["pageId"], "d0");
    assert_eq!(
        json::value(&merged, "tags").unwrap()[0]["timestamp"].to_string(),
        "9007199254740993"
    );
    let mut conflict = dst.clone();
    conflict.insert(
        "tags".into(),
        json::raw(&serde_json::json!([{"name":"other","timestamp":1}])).unwrap(),
    );
    assert!(settings::merge(&src, &conflict, &map, true, &mut Vec::new()).is_err());
    assert!(settings::merge(&src, &dst, &[], true, &mut Vec::new()).is_err());
    assert!(
        settings::merge(
            &src,
            &dst,
            &[PageMap {
                source_page: 2,
                destination_page: 0
            }],
            true,
            &mut Vec::new()
        )
        .is_err()
    );
    for (key, value) in [
        ("zoomMode", "\"unsupported\""),
        ("customZoomScale", "1e9999"),
        ("customZoomScale", "null"),
        ("customZoomOrientation", "\"diagonal\""),
        ("viewBackgroundFilter", "\"all\""),
    ] {
        let mut invalid = src.clone();
        invalid.insert(
            key.into(),
            serde_json::value::RawValue::from_string(value.into()).unwrap(),
        );
        assert!(settings::merge(&invalid, &dst, &map, true, &mut Vec::new()).is_err());
    }
    assert!(
        settings::validate_map(&[
            PageMap {
                source_page: 0,
                destination_page: 0
            },
            PageMap {
                source_page: 0,
                destination_page: 1
            }
        ])
        .is_err()
    );
}

#[test]
fn search_matches_simple_unicode_case_and_whole_word_boundaries() {
    assert!(search::snippet("Σ ς σ", "σ", true).is_some());
    assert!(search::snippet("Kelvin", "kelvin", true).is_some());
    assert!(search::snippet("long ſ", "s", true).is_some());
    assert!(search::snippet("concatenate", "cat", true).is_none());
    assert!(search::snippet("cat\u{301}", "cat", true).is_none());
    assert!(search::snippet("prefix cat suffix", "cat", true).is_some());
    assert!(search::snippet("line\n \tbreak", "line break", false).is_some());
    assert!(search::snippet("nothing", " ", false).is_none());
    assert!(search::snippet("a", "long", false).is_none());
    assert!(search::snippet("cat cat", "cat", false).is_some());
}

#[test]
fn path_and_offline_input_validation_prevent_escape_and_preserve_names() {
    for title in ["..", ".", "", "  "] {
        assert_eq!(v::safe_document_dir(title, "uuid"), "uuid");
    }
    assert_eq!(v::safe_document_dir(" a/b\\c ", "uuid"), "a-b-c");
    for path in ["../a", "a/../b", "/a", "a//b", "a\\b", "a:b", "a\0b"] {
        assert!(!v::safe_native_path(path));
    }
    assert!(v::safe_native_path("id/page.rm"));
    assert!(v::title("line\nbreak").is_err());
    assert!(v::title(" ").is_err());
    assert!(v::title("Title").is_ok());
    assert!(v::query("\t").is_err());
    assert!(v::pairing_code(" short ").is_err());
    assert_eq!(v::pairing_code(" 12345678 ").unwrap(), "12345678");
    assert_eq!(v::cat_format(" PDF ").unwrap(), "pdf");
    assert!(v::cat_format("png").is_err());
    assert!(v::sync_format("SVG").is_err());
    assert_eq!(v::sync_format("").unwrap(), "png");
    assert!(v::folder("").is_ok());
    assert!(v::folder("bad").is_err());
    assert!(v::nonempty("").is_err());
}

#[test]
fn output_modes_format_trees_status_and_tables_and_propagate_errors() {
    let tree = TreeNode {
        label: "/".into(),
        children: vec![
            TreeNode {
                label: "one".into(),
                children: vec![TreeNode {
                    label: "nested".into(),
                    children: vec![],
                }],
            },
            TreeNode {
                label: "two".into(),
                children: vec![],
            },
        ],
    };
    for mode in [Mode::Agent, Mode::Human] {
        let output = Output::new(mode);
        let mut bytes = Vec::new();
        output.tree(&mut bytes, &tree).unwrap();
        output.status(&mut bytes, "warning", "care").unwrap();
        output
            .key_values(&mut bytes, &[("name", "value\n".into())])
            .unwrap();
        output
            .table(&mut bytes, &[], &[vec!["cell".into()]])
            .unwrap();
        output.separator(&mut bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(if mode == Mode::Agent {
            "  * one"
        } else {
            "├── one"
        }));
        assert!(text.contains("value\\n"));
    }
}

#[test]
fn typed_error_precedence_and_commit_state_survive_context() {
    let missing = || Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
    assert_eq!(missing().context("read").exit_status(), 3);
    assert_eq!(Error::Credentials("path".into()).exit_status(), 4);
    assert_eq!(Error::Deadline.exit_status(), 5);
    assert_eq!(Error::Cancelled.exit_status(), 1);
    for state in [
        UpdateState::Staged,
        UpdateState::CommitUnknown,
        UpdateState::Committed,
        UpdateState::Verified,
    ] {
        assert_eq!(
            after_commit(state, missing()).exit_status(),
            if matches!(state, UpdateState::CommitUnknown | UpdateState::Committed) {
                6
            } else {
                3
            }
        );
    }
    assert_eq!(
        Error::Multiple(vec![
            Error::Deadline,
            Error::Credentials("path".into()),
            missing()
        ])
        .exit_status(),
        4
    );
    assert_eq!(
        Error::Multiple(vec![
            Error::Credentials("path".into()),
            after_commit(UpdateState::Committed, missing())
        ])
        .exit_status(),
        6
    );
}

#[test]
fn native_command_tree_rejects_invalid_offline_flags_before_credentials() {
    commands::raw_command().debug_assert();
    for args in [
        vec!["doc", "cat", "id", "--page=-1"],
        vec!["doc", "cat", "id", "--format=bad"],
        vec!["auth", "pair", "bad"],
        vec!["doc", "search", "id", " "],
        vec!["doc", "upload", "a.pdf", "--title=", "--evidence=e.json"],
        vec!["doc", "import", "bad", "--mapping=m.json"],
    ] {
        assert!(
            commands::raw_command()
                .try_get_matches_from(std::iter::once("remarkable").chain(args))
                .is_err()
        );
    }
    let m = commands::raw_command()
        .try_get_matches_from([
            "remarkable",
            "doc",
            "cat",
            "id",
            "--page=0x10",
            "--debug=false",
        ])
        .unwrap();
    assert_eq!(
        *m.subcommand()
            .unwrap()
            .1
            .subcommand()
            .unwrap()
            .1
            .get_one::<usize>("page")
            .unwrap(),
        16
    );
}

#[test]
fn native_page_order_spans_all_verified_runs_and_rejects_unsupported_sizes() {
    let order = upload::page_order(816).unwrap();
    assert_eq!(order.len(), 816);
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(upload::page_order(1).unwrap(), order[..1]);
    assert!(upload::page_order(0).is_err());
    assert!(upload::page_order(817).is_err());
}
