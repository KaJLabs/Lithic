//! `lithc` — the Lithic compiler.
//!
//! The command exposes declaration checks and fail-closed EVM and native
//! LithoVM backends for the currently supported fail-closed language subset.

use std::process::exit;

fn print_help() {
    eprintln!(
        r#"lithc {} — Lithic compiler

USAGE:
    lithc [OPTIONS] <FILE.lithic>

OPTIONS:
    --emit <KIND>   summary (default), ast, abi, check, evm, bytecode, runtime,
                    lithovm, lithovm-bytecode
    -h, --help      Print this help

EXAMPLES:
    lithc Makalu/contracts/src/DOGE.lithic
    lithc --emit abi DOGE.lithic
    lithc --emit check DOGE.lithic
    lithc --emit evm apps/examples/frontend/evm-constants.lithic
    lithc --emit lithovm apps/examples/frontend/evm-constants.lithic

EVM output supports its documented stateless subset. Native LithoVM v11 also
supports typed constants, gas-bounded repeat loops, mutable and immutable
locals, staged contract-call intents and native transfers, typed events,
scalar and typed map storage, checked u64/u256 expressions, explicit host
context, structured if/else, and transactional require/revert semantics.
Unsupported semantics reject the complete build."#,
        env!("CARGO_PKG_VERSION")
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut path: Option<String> = None;
    let mut emit = String::from("summary");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--emit" => {
                i += 1;
                if i < args.len() {
                    emit = args[i].clone();
                } else {
                    eprintln!("lithc: error: --emit requires a value");
                    exit(2);
                }
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            s if !s.starts_with('-') => path = Some(s.to_string()),
            other => {
                eprintln!("lithc: error: unknown option '{}'", other);
                exit(2);
            }
        }
        i += 1;
    }

    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("lithc: error: no input file given\n");
            print_help();
            exit(2);
        }
    };

    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lithc: error: cannot read {}: {}", path, e);
            exit(2);
        }
    };

    if matches!(emit.as_str(), "evm" | "bytecode" | "runtime") {
        emit_evm(&src, &emit);
        return;
    }

    if matches!(emit.as_str(), "lithovm" | "lithovm-bytecode") {
        emit_lithovm(&src, &emit);
        return;
    }

    let res = lithic_syntax::parse(&src);
    for d in &res.diagnostics {
        eprintln!("{}", d.render(&src, &path));
    }

    let errors = res.error_count();
    let contract = match res.contract {
        Some(c) => c,
        None => {
            eprintln!("lithc: error: no contract found in {}", path);
            exit(1);
        }
    };

    if errors > 0 {
        eprintln!("lithc: aborting due to {} error(s)", errors);
        exit(1);
    }

    let findings = lithic_syntax::check(&contract);
    for finding in &findings {
        eprintln!("{}", finding.render(&path));
    }
    let declaration_errors = lithic_syntax::sema::error_count(&findings);
    if declaration_errors > 0 {
        eprintln!(
            "lithc: aborting due to {} declaration error(s)",
            declaration_errors
        );
        exit(1);
    }

    match emit.as_str() {
        "check" => eprintln!("{}: declaration checks clean", path),
        "summary" => print!("{}", contract.summary()),
        "ast" => println!("{}", contract.to_json()),
        "abi" => println!("{}", contract.to_abi_json()),
        other => {
            eprintln!(
                "lithc: error: unknown emit kind '{}' (expected summary|ast|abi|check|evm|bytecode|runtime|lithovm|lithovm-bytecode)",
                other
            );
            exit(2);
        }
    }
}

fn emit_lithovm(source: &str, emit: &str) {
    match lithic_lithovm::compile(source) {
        Ok(artifact) => match emit {
            "lithovm" => println!("{}", artifact.to_json()),
            "lithovm-bytecode" => println!("{}", artifact.bytecode),
            _ => unreachable!(),
        },
        Err(error) => {
            for message in error.messages() {
                eprintln!("lithc: error: {message}");
            }
            exit(1);
        }
    }
}

fn emit_evm(source: &str, emit: &str) {
    match lithic_evm::compile(source) {
        Ok(artifact) => match emit {
            "evm" => println!("{}", artifact.to_json()),
            "bytecode" => println!("{}", artifact.bytecode),
            "runtime" => println!("{}", artifact.deployed_bytecode),
            _ => unreachable!(),
        },
        Err(error) => {
            for message in error.messages() {
                eprintln!("lithc: error: {message}");
            }
            exit(1);
        }
    }
}
