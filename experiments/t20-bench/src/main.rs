use std::{env, path::PathBuf, process::ExitCode};

use uste_t20_bench::{
    Bm01Manifest, Bm01Profile, OracleBundle, OracleSummary, verify_development_profile,
    verify_disk_development_profile,
};

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let command = arguments
        .next()
        .unwrap_or_else(|| "manifest".into())
        .into_string()
        .map_err(|_| "command must be UTF-8")?;
    if command == "--help" || command == "-h" {
        print_usage();
        return Ok(());
    }
    let linux_command = matches!(
        command.as_str(),
        "linux-create"
            | "linux-disk-create"
            | "linux-disk-resume"
            | "linux-disk-open"
            | "linux-disk-query"
            | "linux-disk-sample"
            | "linux-disk-sample-worker"
            | "linux-disk-create-crash-probe"
            | "linux-create-crash-probe"
            | "linux-resume"
            | "linux-open"
            | "linux-query"
            | "linux-sample"
            | "linux-sample-worker"
    );
    if command != "manifest"
        && command != "oracle-summary"
        && command != "oracle-bundle"
        && command != "engine-check"
        && command != "disk-engine-check"
        && !linux_command
    {
        return Err("unsupported command".into());
    }
    let mut entities = 100_000_u64;
    let mut root = None::<PathBuf>;
    let mut password_file = None::<PathBuf>;
    let mut oracle_file = None::<PathBuf>;
    let mut pause_after_revision = None::<u64>;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--entities") => {
                entities = arguments
                    .next()
                    .ok_or("--entities requires a value")?
                    .into_string()
                    .map_err(|_| "--entities must be UTF-8")?
                    .parse()
                    .map_err(|_| "--entities must be an unsigned integer")?;
            }
            Some("--root") => {
                root = Some(PathBuf::from(
                    arguments.next().ok_or("--root requires a value")?,
                ));
            }
            Some("--password-file") => {
                password_file = Some(PathBuf::from(
                    arguments.next().ok_or("--password-file requires a value")?,
                ));
            }
            Some("--oracle-file") => {
                oracle_file = Some(PathBuf::from(
                    arguments.next().ok_or("--oracle-file requires a value")?,
                ));
            }
            Some("--pause-after-revision") => {
                pause_after_revision = Some(
                    arguments
                        .next()
                        .ok_or("--pause-after-revision requires a value")?
                        .into_string()
                        .map_err(|_| "--pause-after-revision must be UTF-8")?
                        .parse()
                        .map_err(|_| "--pause-after-revision must be an unsigned integer")?,
                );
            }
            Some("--help" | "-h") => {
                print_usage();
                return Ok(());
            }
            _ => return Err("unsupported argument".into()),
        }
    }
    let profile = Bm01Profile::new(entities).map_err(str::to_owned)?;
    if command == "manifest" {
        print!("{}", Bm01Manifest::build(profile).to_json());
    } else if command == "oracle-summary" {
        print!("{}", OracleSummary::build(profile)?.to_tsv());
    } else if command == "oracle-bundle" {
        print!("{}", OracleBundle::build(profile)?.to_tsv());
    } else if command == "disk-engine-check" {
        let report = verify_disk_development_profile(profile)?;
        println!(
            "{{\"engine_benchmark\":false,\"qualification\":\"nonqualifying-disk-development-equivalence\",\"filesystem_profile\":\"durable-memory-model\",\"oracle_memory_resident\":true,\"full_memory_graph_state\":false,\"full_memory_coordinator_metadata\":false,\"storage_metadata_memory_resident\":true,\"entities\":{},\"relationships\":{},\"recovered_revision\":{},\"queries\":{},\"output_digest\":\"{}\",\"cache_budget_bytes\":{},\"cache_hits\":{},\"cache_misses\":{}}}",
            report.entities,
            report.relationships,
            report.recovered_revision,
            report.queries,
            hex(&report.output_digest),
            report.cache_report.budget_bytes,
            report.cache_report.hits,
            report.cache_report.misses,
        );
    } else if command == "engine-check" {
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
    } else {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let root = root.ok_or("--root is required")?;
            let password_file = password_file.ok_or("--password-file is required")?;
            if command == "linux-sample-worker" || command == "linux-disk-sample-worker" {
                uste_t20_bench::linux_runner::start_parent_watchdog()
                    .map_err(|error| error.code().to_owned())?;
                let worker = if command == "linux-disk-sample-worker" {
                    uste_t20_bench::linux_runner::disk::sample_worker
                } else {
                    uste_t20_bench::linux_runner::sample_worker
                };
                worker(
                    &root,
                    &password_file,
                    &oracle_file.ok_or("--oracle-file is required")?,
                    profile,
                )
                .map_err(|error| error.code().to_owned())?;
                return Ok(());
            }
            let report = match command.as_str() {
                "linux-disk-sample" => {
                    let executable = env::current_exe()
                        .map_err(|_| "cannot resolve current benchmark executable")?;
                    uste_t20_bench::linux_runner::supervise_disk_sample(
                        &executable,
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                "linux-disk-create-crash-probe" => {
                    uste_t20_bench::linux_runner::disk::create_crash_probe(
                        &root,
                        &password_file,
                        profile,
                        pause_after_revision.ok_or("--pause-after-revision is required")?,
                    )
                }
                "linux-disk-query" => uste_t20_bench::linux_runner::disk::query_correctness(
                    &root,
                    &password_file,
                    &oracle_file.ok_or("--oracle-file is required")?,
                    profile,
                ),
                "linux-disk-create" | "linux-disk-resume" | "linux-disk-open" => {
                    uste_t20_bench::linux_runner::disk::run(
                        &root,
                        &password_file,
                        profile,
                        command
                            .strip_prefix("linux-disk-")
                            .expect("validated disk command"),
                    )
                }
                "linux-create" => {
                    uste_t20_bench::linux_runner::create(&root, &password_file, profile)
                        .map(|report| report.to_json())
                }
                "linux-create-crash-probe" => uste_t20_bench::linux_runner::create_crash_probe(
                    &root,
                    &password_file,
                    profile,
                    pause_after_revision.ok_or("--pause-after-revision is required")?,
                )
                .map(|report| report.to_json()),
                "linux-resume" => {
                    uste_t20_bench::linux_runner::resume(&root, &password_file, profile)
                        .map(|report| report.to_json())
                }
                "linux-open" => {
                    uste_t20_bench::linux_runner::validate_open(&root, &password_file, profile)
                        .map(|report| report.to_json())
                }
                "linux-query" => uste_t20_bench::linux_runner::query_correctness(
                    &root,
                    &password_file,
                    &oracle_file.ok_or("--oracle-file is required")?,
                    profile,
                )
                .map(|report| report.to_json()),
                "linux-sample" => {
                    let executable = env::current_exe()
                        .map_err(|_| "cannot resolve current benchmark executable")?;
                    uste_t20_bench::linux_runner::supervise_sample(
                        &executable,
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                _ => unreachable!("command was validated"),
            }
            .map_err(|error| error.code().to_owned())?;
            println!("{report}");
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            let _ = (
                root,
                password_file,
                oracle_file,
                pause_after_revision,
                profile,
            );
            return Err("linux runner requires x86_64 Linux".into());
        }
    }
    Ok(())
}

fn print_usage() {
    println!(
        "usage: uste-t20-bench <manifest|oracle-summary|oracle-bundle|engine-check|disk-engine-check> [--entities COUNT]\n\
         uste-t20-bench <linux-create|linux-resume|linux-open> \
         --root DIR --password-file FILE [--entities COUNT]\n\
         uste-t20-bench <linux-disk-create|linux-disk-resume|linux-disk-open> \
         --root DIR --password-file FILE --entities COUNT (at most 1000; nonqualifying)\n\
         uste-t20-bench linux-create-crash-probe --root DIR --password-file FILE \
         --pause-after-revision REVISION [--entities COUNT]\n\
         uste-t20-bench linux-disk-create-crash-probe --root DIR --password-file FILE \
         --pause-after-revision REVISION --entities COUNT (at most 1000; nonqualifying)\n\
         uste-t20-bench <linux-query|linux-sample> --root DIR --password-file FILE \
         --oracle-file FILE [--entities COUNT]\n\
         uste-t20-bench <linux-disk-query|linux-disk-sample> --root DIR --password-file FILE \
         --oracle-file FILE --entities COUNT (at most 1000; nonqualifying)\n\
         default COUNT=100000 creates the exact qualifying-size fixture manifest;\n\
         oracle-summary emits content-free expectations for a separate query process;\n\
         oracle-bundle emits disjoint warm-up plus measured expectations for sampling;\n\
         linux-sample uses one development round at scaled sizes and a fixed qualifying\n\
         plan of one warm-up plus five samples of at least 60 seconds at exact size;\n\
         both engine checks accept at most 1000 entities and are always nonqualifying;\n\
         Linux phases require an existing Btrfs directory and owner-only password file"
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
