pub mod pipeline;

use clap::{Parser, Subcommand};
use kermlc_diagnostics::{render_diagnostics, DiagnosticSink, SourceMap};
use kermlc_hir::{load_stdlib, lower_ast};
use kermlc_intern::StringInterner;
use kermlc_serial_json::serialize_to_json;
use kermlc_validate::validate;
use std::process;

use crate::pipeline::resolve_and_typecheck;

#[derive(Parser)]
#[command(name = "kermlc", about = "KerML compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse, resolve, and validate a KerML file
    Check {
        /// Path to the KerML source file
        file: String,
        /// Path to stdlib directory (optional; uses built-in minimal stdlib if omitted)
        #[arg(long)]
        stdlib: Option<String>,
    },
    /// Compile a KerML file to output format
    Compile {
        /// Path to the KerML source file
        file: String,
        /// Output file path
        #[arg(short, long)]
        output: String,
        /// Output format (json)
        #[arg(short, long, default_value = "json")]
        format: String,
        /// Path to stdlib directory (optional)
        #[arg(long)]
        stdlib: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { file, stdlib: _ } => {
            let exit_code = run_check(&file);
            process::exit(exit_code);
        }
        Command::Compile {
            file,
            output,
            format,
            stdlib: _,
        } => {
            let exit_code = run_compile(&file, &output, &format);
            process::exit(exit_code);
        }
    }
}

fn run_check(file_path: &str) -> i32 {
    let source = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read `{}`: {}", file_path, e);
            return 1;
        }
    };

    let mut interner = StringInterner::new();
    let mut source_map = SourceMap::new();
    let mut sink = DiagnosticSink::new();

    let file_id = source_map.add_file(file_path.to_string(), source.clone());

    // Parse
    let parse = kermlc_parser::Parser::parse(&source, file_id, &mut interner, &mut sink);

    // Lower to HIR
    let mut model = lower_ast(&parse, &interner, &mut sink);

    // Load stdlib
    load_stdlib(&mut model, &mut interner);

    // Resolve + typecheck
    resolve_and_typecheck(&mut model, &interner, &mut sink);

    // Validate
    validate(&model, &interner, &mut sink);

    // Render diagnostics
    let diagnostics = sink.diagnostics();
    if !diagnostics.is_empty() {
        render_diagnostics(&source_map, diagnostics);
    }

    if sink.has_errors() {
        1
    } else {
        0
    }
}

fn run_compile(file_path: &str, output_path: &str, format: &str) -> i32 {
    if format != "json" {
        eprintln!("error: unsupported format `{}`, only `json` is supported", format);
        return 1;
    }

    let source = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read `{}`: {}", file_path, e);
            return 1;
        }
    };

    let mut interner = StringInterner::new();
    let mut source_map = SourceMap::new();
    let mut sink = DiagnosticSink::new();

    let file_id = source_map.add_file(file_path.to_string(), source.clone());

    // Parse
    let parse = kermlc_parser::Parser::parse(&source, file_id, &mut interner, &mut sink);

    // Lower to HIR
    let mut model = lower_ast(&parse, &interner, &mut sink);

    // Load stdlib
    load_stdlib(&mut model, &mut interner);

    // Resolve + typecheck
    resolve_and_typecheck(&mut model, &interner, &mut sink);

    // Validate
    validate(&model, &interner, &mut sink);

    // Render diagnostics
    let diagnostics = sink.diagnostics();
    if !diagnostics.is_empty() {
        render_diagnostics(&source_map, diagnostics);
    }

    if sink.has_errors() {
        return 1;
    }

    // Serialize
    let json = serialize_to_json(&model, &interner);

    match std::fs::write(output_path, &json) {
        Ok(()) => {
            eprintln!("wrote {}", output_path);
            0
        }
        Err(e) => {
            eprintln!("error: could not write `{}`: {}", output_path, e);
            1
        }
    }
}
