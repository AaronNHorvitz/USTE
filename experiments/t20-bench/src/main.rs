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
    let packed_history = command.starts_with("bm06-packed-linux-");
    if let Some(phase) = command
        .strip_prefix("bm06-linux-")
        .or_else(|| command.strip_prefix("bm06-packed-linux-"))
    {
        let mut root = None;
        let mut password = None;
        let mut records = None;
        let mut pause = None;
        let mut through = None;
        while let Some(flag) = arguments.next() {
            let value = arguments
                .next()
                .ok_or("BM-06 native flag requires a value")?;
            match flag.to_str() {
                Some("--root") if root.is_none() => root = Some(PathBuf::from(value)),
                Some("--password-file") if password.is_none() => {
                    password = Some(PathBuf::from(value))
                }
                Some("--records") if records.is_none() => {
                    records = Some(
                        value
                            .into_string()
                            .map_err(|_| "BM-06 record count must be UTF-8")?
                            .parse::<u64>()
                            .map_err(|_| "BM-06 record count must be unsigned")?,
                    )
                }
                Some("--pause-after-revision") if pause.is_none() => {
                    pause = Some(
                        value
                            .into_string()
                            .map_err(|_| "BM-06 pause must be UTF-8")?
                            .parse::<u64>()
                            .map_err(|_| "BM-06 pause must be unsigned")?,
                    );
                }
                Some("--through-revision") if through.is_none() => {
                    through = Some(
                        value
                            .into_string()
                            .map_err(|_| "BM-06 target must be UTF-8")?
                            .parse::<u64>()
                            .map_err(|_| "BM-06 target must be unsigned")?,
                    );
                }
                _ => return Err("unsupported or duplicate BM-06 native flag".into()),
            }
        }
        let root = root.ok_or("BM-06 native command requires --root")?;
        if (packed_history && matches!(phase, "create-prefix" | "resume-prefix"))
            != through.is_some()
        {
            return Err(
                "packed construction prefixes require --through-revision; other phases refuse it"
                    .into(),
            );
        }
        let password = password.ok_or("BM-06 native command requires --password-file")?;
        let profile = uste_t20_bench::recovery_materialization::Bm06Profile::new(
            records.ok_or("BM-06 native command requires --records")?,
        )?;
        if (phase == "create-crash-probe" || (packed_history && phase == "tail-prefix-crash-probe"))
            != pause.is_some()
        {
            return Err(
                "BM-06 prefix crash probes require --pause-after-revision; other phases refuse it"
                    .into(),
            );
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let report = if packed_history {
                if let Some(target) = through {
                    uste_t20_bench::linux_runner::packed::history::construct_prefix(
                        &root,
                        &password,
                        profile,
                        target,
                        phase == "create-prefix",
                    )
                } else if let Some(pause) = pause {
                    if phase == "create-crash-probe" {
                        uste_t20_bench::linux_runner::packed::history::create_crash_probe(
                            &root, &password, profile, pause,
                        )
                    } else {
                        uste_t20_bench::linux_runner::packed::history::tail_prefix_crash_probe(
                            &root, &password, profile, pause,
                        )
                    }
                } else {
                    uste_t20_bench::linux_runner::packed::history::run(
                        &root, &password, profile, phase,
                    )
                }
            } else if let Some(pause) = pause {
                uste_t20_bench::linux_runner::disk::recovery::create_crash_probe(
                    &root, &password, profile, pause,
                )
            } else {
                uste_t20_bench::linux_runner::disk::recovery::run(&root, &password, profile, phase)
            };
            println!("{}", report.map_err(|error| error.to_string())?);
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            let _ = (root, password, profile, phase, pause, through);
            return Err("BM-06 native runner requires x86_64 Linux".into());
        }
        return Ok(());
    }
    if matches!(
        command.as_str(),
        "bm06-manifest" | "bm06-disk-check" | "bm06-packed-check"
    ) {
        let records = match arguments.next() {
            None => 100_000,
            Some(flag) if flag == "--records" => arguments
                .next()
                .ok_or("--records requires a value")?
                .into_string()
                .map_err(|_| "--records must be UTF-8")?
                .parse::<u64>()
                .map_err(|_| "--records must be an unsigned integer")?,
            _ => return Err("BM-06 commands accept only --records N".into()),
        };
        if arguments.next().is_some() {
            return Err("unexpected BM-06 argument".into());
        }
        let profile = uste_t20_bench::recovery_materialization::Bm06Profile::new(records)?;
        if command == "bm06-manifest" {
            print!("{}", profile.manifest());
        } else if command == "bm06-packed-check" {
            let report = uste_t20_bench::engine::packed::recovery::verify(profile)?;
            println!(
                "{}",
                serde_json::json!({
                    "schema": "bm06-packed-development-v1", "engine_benchmark": false,
                    "qualification": "nonqualifying-packed-development-equivalence",
                    "filesystem_profile": "durable-memory-model", "complete_authenticated_io": false,
                    "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
                    "records": report.records, "verified_versions": report.verified_versions,
                    "verified_payload_bytes": report.verified_payload_bytes,
                    "base_revision": report.base_revision, "recovered_revision": report.recovered_revision,
                    "origin_suffix_groups": report.origin_suffix_groups, "v1_state_digest": hex(&report.v1_state_digest),
                    "qualifying_recovery_trials": 0,
                })
            );
        } else {
            let report = uste_t20_bench::engine::recovery::verify_disk_recovery(profile)?;
            println!(
                "{{\"engine_benchmark\":false,\"qualification\":\"nonqualifying-disk-development-equivalence\",\"filesystem_profile\":\"durable-memory-model\",\"full_memory_graph_state\":false,\"full_memory_coordinator_metadata\":false,\"storage_metadata_memory_resident\":{},\"records\":{},\"verified_versions\":{},\"verified_payload_bytes\":{},\"base_revision\":{},\"recovered_revision\":{},\"qualifying_recovery_trials\":0}}",
                report.storage_metadata_memory_resident,
                report.records,
                report.verified_versions,
                report.verified_payload_bytes,
                report.base_revision,
                report.recovered_revision
            );
        }
        return Ok(());
    }
    let linux_command = matches!(
        command.as_str(),
        "linux-create"
            | "linux-disk-create"
            | "linux-packed-create"
            | "linux-packed-create-crash-probe"
            | "linux-packed-open"
            | "linux-packed-resume"
            | "linux-packed-rebuild"
            | "linux-packed-query"
            | "linux-packed-lookup-query"
            | "linux-packed-wide-query"
            | "linux-packed-wide-lookup-query"
            | "linux-packed-sample"
            | "linux-packed-sample-worker"
            | "linux-packed-lookup-sample"
            | "linux-packed-lookup-sample-worker"
            | "linux-packed-wide-sample"
            | "linux-packed-wide-sample-worker"
            | "linux-packed-wide-lookup-sample"
            | "linux-packed-wide-lookup-sample-worker"
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
        && command != "packed-engine-check"
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
            "{{\"engine_benchmark\":false,\"qualification\":\"nonqualifying-disk-development-equivalence\",\"filesystem_profile\":\"durable-memory-model\",\"oracle_memory_resident\":true,\"full_memory_graph_state\":false,\"full_memory_coordinator_metadata\":false,\"storage_metadata_memory_resident\":{},\"entities\":{},\"relationships\":{},\"recovered_revision\":{},\"queries\":{},\"output_digest\":\"{}\",\"cache_budget_bytes\":{},\"cache_hits\":{},\"cache_misses\":{}}}",
            report.storage_metadata_memory_resident,
            report.entities,
            report.relationships,
            report.recovered_revision,
            report.queries,
            hex(&report.output_digest),
            report.cache_report.budget_bytes,
            report.cache_report.hits,
            report.cache_report.misses,
        );
    } else if command == "packed-engine-check" {
        let report = uste_t20_bench::engine::packed::verify_packed_development_profile(profile)?;
        println!(
            "{{\"engine_benchmark\":false,\"qualification\":\"nonqualifying-packed-development-equivalence\",\"filesystem_profile\":\"durable-memory-model\",\"oracle_memory_resident\":true,\"complete_authenticated_io\":false,\"entities\":{},\"relationships\":{},\"recovered_revision\":{},\"origin_suffix_groups\":{},\"queries\":{},\"output_digest\":\"{}\",\"v1_state_digest\":\"{}\",\"cache_budget_bytes\":{},\"cache_accounted_bytes\":{},\"cache_hits\":{},\"cache_misses\":{},\"cache_evictions\":{}}}",
            report.entities,
            report.relationships,
            report.recovered_revision,
            report.origin_suffix_groups,
            report.queries,
            hex(&report.output_digest),
            hex(&report.v1_state_digest),
            report.cache_report.budget_bytes,
            report.cache_report.accounted_bytes,
            report.cache_report.hits,
            report.cache_report.misses,
            report.cache_report.evictions,
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
            if matches!(
                command.as_str(),
                "linux-sample-worker"
                    | "linux-disk-sample-worker"
                    | "linux-packed-sample-worker"
                    | "linux-packed-lookup-sample-worker"
                    | "linux-packed-wide-sample-worker"
                    | "linux-packed-wide-lookup-sample-worker"
            ) {
                uste_t20_bench::linux_runner::start_parent_watchdog()
                    .map_err(|error| error.code().to_owned())?;
                let worker = if command == "linux-packed-wide-sample-worker" {
                    uste_t20_bench::linux_runner::packed::sample_worker_wide
                } else if command == "linux-packed-wide-lookup-sample-worker" {
                    uste_t20_bench::linux_runner::packed::sample_worker_wide_with_lookup
                } else if command == "linux-packed-lookup-sample-worker" {
                    uste_t20_bench::linux_runner::packed::sample_worker_with_lookup
                } else if command == "linux-packed-sample-worker" {
                    uste_t20_bench::linux_runner::packed::sample_worker
                } else if command == "linux-disk-sample-worker" {
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
                "linux-packed-wide-sample" | "linux-packed-wide-lookup-sample" => {
                    let executable = env::current_exe()
                        .map_err(|_| "cannot resolve current benchmark executable")?;
                    let supervisor = if command == "linux-packed-wide-sample" {
                        uste_t20_bench::linux_runner::supervise_packed_wide_sample
                    } else {
                        uste_t20_bench::linux_runner::supervise_packed_wide_lookup_sample
                    };
                    supervisor(
                        &executable,
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                "linux-packed-lookup-sample" => {
                    let executable = env::current_exe()
                        .map_err(|_| "cannot resolve current benchmark executable")?;
                    uste_t20_bench::linux_runner::supervise_packed_lookup_sample(
                        &executable,
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                "linux-packed-sample" => {
                    let executable = env::current_exe()
                        .map_err(|_| "cannot resolve current benchmark executable")?;
                    uste_t20_bench::linux_runner::supervise_packed_sample(
                        &executable,
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                "linux-packed-create-crash-probe" => {
                    uste_t20_bench::linux_runner::packed::create_crash_probe(
                        &root,
                        &password_file,
                        profile,
                        pause_after_revision.ok_or("--pause-after-revision is required")?,
                    )
                }
                "linux-packed-create"
                | "linux-packed-open"
                | "linux-packed-rebuild"
                | "linux-packed-resume" => uste_t20_bench::linux_runner::packed::run(
                    &root,
                    &password_file,
                    profile,
                    command
                        .strip_prefix("linux-packed-")
                        .ok_or("invalid packed command")?,
                ),
                "linux-packed-query" => uste_t20_bench::linux_runner::packed::query_correctness(
                    &root,
                    &password_file,
                    &oracle_file.ok_or("--oracle-file is required")?,
                    profile,
                ),
                "linux-packed-lookup-query" => {
                    uste_t20_bench::linux_runner::packed::query_correctness_with_lookup(
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                "linux-packed-wide-query" => {
                    uste_t20_bench::linux_runner::packed::query_correctness_wide(
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
                "linux-packed-wide-lookup-query" => {
                    uste_t20_bench::linux_runner::packed::query_correctness_wide_with_lookup(
                        &root,
                        &password_file,
                        &oracle_file.ok_or("--oracle-file is required")?,
                        profile,
                    )
                }
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
        "usage: uste-t20-bench <manifest|oracle-summary|oracle-bundle|engine-check|disk-engine-check|packed-engine-check> [--entities COUNT]\n\
         uste-t20-bench bm06-manifest [--records COUNT] (fixture only; no recovery benchmark)\n\
         uste-t20-bench bm06-disk-check --records COUNT (at most 2; memory-model equivalence only)\n\
         uste-t20-bench bm06-packed-check --records COUNT (at most 2; memory-model equivalence only)\n\
         uste-t20-bench bm06-packed-linux-<create|open|tail|recover|recover-checkpoint|rebuild|resume|tail-crash-probe> --root ROOT --password-file PASSWORD --records COUNT (at most 8192; nonqualifying)\n\
         uste-t20-bench bm06-packed-linux-create-crash-probe --root ROOT --password-file PASSWORD --records COUNT --pause-after-revision REVISION (owned-child test control)\n\
         uste-t20-bench bm06-packed-linux-tail-prefix-crash-probe --root ROOT --password-file PASSWORD --records COUNT --pause-after-revision REVISION (owned-child test control)\n\
         uste-t20-bench bm06-packed-linux-<create-prefix|resume-prefix> --root ROOT --password-file PASSWORD --records COUNT --through-revision REVISION (complete-generation construction step)\n\
         uste-t20-bench linux-packed-<create|open|rebuild|resume|query> --root ROOT --password-file PASSWORD --entities COUNT [--oracle-file ORACLE] (nonqualifying)\n\
         uste-t20-bench linux-packed-lookup-query --root ROOT --password-file PASSWORD --entities COUNT --oracle-file ORACLE (nonqualifying; 48 MiB pages + 16 MiB positive lookups)\n\
         uste-t20-bench linux-packed-wide-query --root ROOT --password-file PASSWORD --entities COUNT --oracle-file ORACLE (nonqualifying; 256 MiB pages)\n\
         uste-t20-bench linux-packed-wide-lookup-query --root ROOT --password-file PASSWORD --entities COUNT --oracle-file ORACLE (nonqualifying; 128 MiB pages + 128 MiB positive lookups)\n\
         uste-t20-bench linux-packed-create-crash-probe --root ROOT --password-file PASSWORD --entities COUNT --pause-after-revision REVISION (owned-child test control)\n\
         uste-t20-bench linux-packed-sample --root ROOT --password-file PASSWORD --entities COUNT --oracle-file BUNDLE (supervised, nonqualifying development sampling)\n\
         uste-t20-bench linux-packed-lookup-sample --root ROOT --password-file PASSWORD --entities COUNT --oracle-file BUNDLE (supervised, nonqualifying; 48 MiB pages + 16 MiB positive lookups)\n\
         uste-t20-bench linux-packed-wide-sample --root ROOT --password-file PASSWORD --entities COUNT --oracle-file BUNDLE (supervised, nonqualifying; 256 MiB pages)\n\
         uste-t20-bench linux-packed-wide-lookup-sample --root ROOT --password-file PASSWORD --entities COUNT --oracle-file BUNDLE (supervised, nonqualifying; 128 MiB pages + 128 MiB positive lookups)\n\
         uste-t20-bench bm06-linux-<create|resume|tail|recover|rebuild|open|tail-crash-probe> \
         --root DIR --password-file FILE --records COUNT (at most 2; nonqualifying)\n\
         uste-t20-bench bm06-linux-create-crash-probe --root DIR --password-file FILE \
         --records COUNT --pause-after-revision REVISION\n\
         uste-t20-bench <linux-create|linux-resume|linux-open> \
         --root DIR --password-file FILE [--entities COUNT]\n\
         uste-t20-bench <linux-disk-create|linux-disk-resume|linux-disk-open> \
         --root DIR --password-file FILE --entities COUNT (at most 10000; nonqualifying)\n\
         uste-t20-bench linux-create-crash-probe --root DIR --password-file FILE \
         --pause-after-revision REVISION [--entities COUNT]\n\
         uste-t20-bench linux-disk-create-crash-probe --root DIR --password-file FILE \
         --pause-after-revision REVISION --entities COUNT (at most 10000; nonqualifying)\n\
         uste-t20-bench <linux-query|linux-sample> --root DIR --password-file FILE \
         --oracle-file FILE [--entities COUNT]\n\
         uste-t20-bench <linux-disk-query|linux-disk-sample> --root DIR --password-file FILE \
         --oracle-file FILE --entities COUNT (at most 10000; nonqualifying)\n\
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
