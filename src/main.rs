use std::io;
#[tokio::main]
async fn main() {
    let code = remarkable_cli::runtime::execute(
        std::env::args_os(),
        cli_command_kit::Mode::from_env(),
        &mut io::stdout(),
        &mut io::stderr(),
    )
    .await;
    std::process::exit(i32::from(code));
}
