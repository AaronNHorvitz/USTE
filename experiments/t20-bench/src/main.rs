use std::{env, process::ExitCode};

use uste_t20_bench::{Bm01Manifest, Bm01Profile, verify_development_profile};

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "manifest".into());
    if command == "--help" || command == "-h" {
        print_usage();
        return Ok(());
    }
    if command != "manifest" && command != "engine-check" {
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
    if command == "manifest" {
        print!("{}", Bm01Manifest::build(profile).to_json());
    } else {
        let report = verify_development_profile(profile)?;
        println!(
            "{{\"engine_benchmark\":false,\"qualification\":\"nonqualifying-development-equivalence\",\"entities\":{},\"relationships\":{},\"recovered_revision\":{},\"queries\":{},\"output_digest\":\"{}\",\"authorized_reads\":{},\"index_operations\":{},\"pages_read\":{},\"cache_hits\":{},\"cache_misses\":{}}}",
            report.entities,
            report.relationships,
            report.recovered_revision,
            report.queries,
            hex(&report.output_digest),
            report.cache_report.completed_authorized_reads,
            report.cache_report.completed_index_operations,
            report.cache_report.pages_read,
            report.cache_report.hits,
            report.cache_report.misses,
        );
    }
    Ok(())
}

fn print_usage() {
    println!(
        "usage: uste-t20-bench <manifest|engine-check> [--entities COUNT]\n\
         default COUNT=100000 creates the exact qualifying-size fixture manifest;\n\
         engine-check accepts at most 1000 entities and is always nonqualifying"
    );
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
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
