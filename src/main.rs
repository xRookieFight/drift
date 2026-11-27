// Drift entry point. The Servo shell lands in the next step, for now this just keeps CI honest.
fn main() {
    println!("drift {}", env!("CARGO_PKG_VERSION"));
}
