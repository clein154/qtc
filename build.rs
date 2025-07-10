fn main() {
    // QTC now uses a pure Rust RandomX-like implementation
    // No external dependencies required for RandomX
    println!("cargo:warning=QTC is using a pure Rust RandomX-like implementation");
    println!("cargo:warning=For production use, consider integrating with the official RandomX library");
}
