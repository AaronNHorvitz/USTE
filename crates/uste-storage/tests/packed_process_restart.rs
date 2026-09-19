#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

#[path = "packed_process/support.rs"]
mod support;

#[test]
fn packed_tree_process_sigkill_preserves_old_and_synced_new_roots() {
    support::run();
}
