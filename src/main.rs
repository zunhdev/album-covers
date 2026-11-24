use std::process::ExitCode;

use album_covers::cli::Cli;
use album_covers::progress::Spinner;
use album_covers::{Config, run};
use anstyle::{AnsiColor, Style};
use clap::Parser;

const ERROR: Style = AnsiColor::Red.on_default().bold();

fn main() -> ExitCode {
    // Invalid arguments exit here with status 2, like other command-line tools.
    let cli = Cli::parse();
    cli.color_choice().write_global();

    let spinner = Spinner::new();
    let result = Config::standard().and_then(|config| {
        run(
            &cli,
            &config,
            &mut anstream::stdout().lock(),
            &mut anstream::stderr(),
            &spinner,
        )
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        // `covers … | head` closes the pipe early; that's not an error worth reporting.
        Err(err) if err.is_broken_pipe() => ExitCode::SUCCESS,
        Err(err) => {
            anstream::eprintln!("{ERROR}error:{ERROR:#} {err}");
            ExitCode::FAILURE
        }
    }
}
