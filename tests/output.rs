use cli_command_kit::Mode;
use remarkable_cli::output::{Output, printable_text, printable_uri};

#[test]
fn agent_rows_escape_controls_and_invalid_bytes_without_changing_binary_paths() {
    assert_eq!(printable_text(b"line\t\x1b\\\xff"), "line\\t\\x1b\\\\\\xff");
    assert_eq!(
        printable_text("a\u{202e}\u{a0}b".as_bytes()),
        "a\\u202e\\u00a0b"
    );
    assert_eq!(
        printable_uri(b"https://x/a b\n\xff?x=%20"),
        "https://x/a%20b%0A%FF?x=%20"
    );
    let mut bytes = Vec::new();
    Output::new(Mode::Agent)
        .table(&mut bytes, &["NAME"], &[vec!["a\tb".into()]])
        .unwrap();
    assert_eq!(bytes, b"NAME\na\\tb\n");
}

#[test]
fn errors_writing_formatted_output_propagate() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for mode in [Mode::Agent, Mode::Human] {
        let output = Output::new(mode);
        assert!(output.status(&mut Broken, "error", "failed").is_err());
        assert!(
            output
                .table(&mut Broken, &["A"], &[vec!["value".into()]])
                .is_err()
        );
        assert!(
            output
                .key_values(&mut Broken, &[("a", "b".into())])
                .is_err()
        );
    }
}
