//! CLI for testing Rust contracts without a web server or ERP integration.

use optimizer_core::{capabilities, parse_request, run_request, validate_request};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;

const HELP: &str = "\
Industrial Cutting Optimizer — Stage 17 (independent Rust REST API available)

USAGE:
  optimizer-cli capabilities
  optimizer-cli validate --input <path|->
  optimizer-cli run --input <path|-> [--output <path>]
  optimizer-cli version
  optimizer-cli --help

EXAMPLES:
  optimizer-cli validate --input examples/system-check.json
  optimizer-cli run --input examples/system-check.json
  optimizer-cli run --input examples/preprocess-model.json --output response.json
  optimizer-cli run --input examples/geometry-vertical-split.json
  optimizer-cli run --input examples/guillotine-vhv.json
  optimizer-cli run --input examples/layout-valid.json
  optimizer-cli run --input examples/generate-multi-board.json
  optimizer-cli run --input examples/plan-full-board-stacks.json
  optimizer-cli run --input examples/generate-stacked-multi-board.json
  optimizer-cli run --input examples/simulate-production-stack-cut.json
  optimizer-cli run --input examples/plan-dynamic-stacks.json
  optimizer-cli run --input examples/schedule-production.json
  cat examples/system-check-minimal.json | optimizer-cli run --input -

EXIT CODES:
  0 success; 2 CLI usage; 3 invalid request; 4 I/O or output failure
";

#[derive(Default)]
struct IoArgs {
    input: Option<String>,
    output: Option<String>,
}

enum CliError {
    Usage(String),
    InvalidRequest(String),
    Io(String),
}

impl CliError {
    fn code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::InvalidRequest(_) => 3,
            Self::Io(_) => 4,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Usage(message) | Self::InvalidRequest(message) | Self::Io(message) => message,
        }
    }
}

fn parse_io_args(args: &[String], allow_output: bool) -> Result<IoArgs, CliError> {
    let mut parsed = IoArgs::default();
    let mut i = 0;

    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .filter(|value| !value.starts_with("--") || value.as_str() == "-")
            .ok_or_else(|| CliError::Usage(format!("missing value for {flag}")))?;

        match flag {
            "--input" if parsed.input.is_none() => parsed.input = Some(value.clone()),
            "--output" if allow_output && parsed.output.is_none() => {
                parsed.output = Some(value.clone())
            }
            _ => return Err(CliError::Usage(format!("unknown or duplicate option: {flag}"))),
        }
        i += 2;
    }

    if parsed.input.is_none() {
        return Err(CliError::Usage("missing required --input <path|->".into()));
    }
    Ok(parsed)
}

fn read_input(input: &str) -> Result<String, CliError> {
    if input == "-" {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|err| CliError::Io(format!("cannot read stdin: {err}")))?;
        Ok(buffer)
    } else {
        fs::read_to_string(input)
            .map_err(|err| CliError::Io(format!("cannot read '{}': {err}", input)))
    }
}

fn write_output(output: Option<&str>, json: &str) -> Result<(), CliError> {
    match output {
        None => {
            println!("{json}");
            Ok(())
        }
        Some(path) => {
            if Path::new(path).is_dir() {
                return Err(CliError::Io(format!("output path '{path}' is a directory")));
            }
            fs::write(path, format!("{json}\n"))
                .map_err(|err| CliError::Io(format!("cannot write '{path}': {err}")))
        }
    }
}

fn execute(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h" | "help") | None => {
            print!("{HELP}");
            Ok(())
        }
        Some("version") if args.len() == 1 => {
            println!("optimizer-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("capabilities") if args.len() == 1 => {
            let output = serde_json::to_string_pretty(&capabilities())
                .map_err(|err| CliError::Io(err.to_string()))?;
            write_output(None, &output)
        }
        Some("validate") => {
            let io = parse_io_args(&args[1..], false)?;
            let input = read_input(io.input.as_deref().unwrap_or("-"))?;
            let request = parse_request(&input)
                .map_err(|err| CliError::InvalidRequest(err.to_string()))?;
            let receipt = validate_request(&request)
                .map_err(|err| CliError::InvalidRequest(err.to_string()))?;
            let output = serde_json::to_string_pretty(&receipt)
                .map_err(|err| CliError::Io(err.to_string()))?;
            write_output(None, &output)
        }
        Some("run") => {
            let io = parse_io_args(&args[1..], true)?;
            let input = read_input(io.input.as_deref().unwrap_or("-"))?;
            let request = parse_request(&input)
                .map_err(|err| CliError::InvalidRequest(err.to_string()))?;
            let response = run_request(request)
                .map_err(|err| CliError::InvalidRequest(err.to_string()))?;
            let output = serde_json::to_string_pretty(&response)
                .map_err(|err| CliError::Io(err.to_string()))?;
            write_output(io.output.as_deref(), &output)
        }
        Some(command) => Err(CliError::Usage(format!("unknown command or invalid arguments: {command}"))),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match execute(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.message());
            eprintln!("Run 'optimizer-cli --help' for usage.");
            ExitCode::from(error.code())
        }
    }
}
