use std::{env, process::ExitCode};

use uste_t20_bench::{Bm01Manifest, Bm01Profile};

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "manifest".into());
    if command == "--help" || command == "-h" {
        print_usage();
        return Ok(());
    }
    if command != "manifest" {
        return Err(format!("unsupported command: {command}"));
    }
    let mut entities = 100_000_u64;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--entities" => {
                entities = arguments
                    .next()
                    .ok_or("--entities requires a value")?
                    .parse()
                    .map_err(|_| "--entities must be an unsigned integer")?;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            _ => return Err(format!("unsupported argument: {argument}")),
        }
    }
    let profile = Bm01Profile::new(entities).map_err(str::to_owned)?;
    print!("{}", Bm01Manifest::build(profile).to_json());
    Ok(())
}

fn print_usage() {
    println!(
        "usage: uste-t20-bench manifest [--entities COUNT]\n\
         default COUNT=100000 creates the exact qualifying-size fixture manifest;\n\
         smaller counts always produce a nonqualifying development manifest"
    );
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("uste-t20-bench: {error}");
            ExitCode::FAILURE
        }
    }
}
