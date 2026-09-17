#![forbid(unsafe_code)]

fn main() {
    // This binary deliberately has no behavior. Its lockfile and resolved feature graph are
    // an R0 feasibility input; production crates admit dependencies separately.
    println!("USTE strict-profile dependency candidates resolve and link");
}
