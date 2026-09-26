use std::io::{Read, Write};
fn main() {
    if std::env::args().any(|arg| arg == "--early-exit") {
        return;
    }
    println!("OBSERVER_CHILD_READY");
    std::io::stdout().flush().expect("flush ready");
    let mut byte = [0u8; 1];
    let _ = std::io::stdin().read(&mut byte);
}
