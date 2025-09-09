//! Stub file generator for the basilica Python module
//!
//! This binary generates Python type stub files (.pyi) for the basilica module.
//! Run with: cargo run --bin stub_gen --features stub-gen

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen::Result;

#[cfg(feature = "stub-gen")]
fn main() -> Result<()> {
    // Get the stub information from the basilica module
    let stub = basilica::stub_info()?;

    // Generate the stub file to python/basilica/_basilica.pyi
    stub.generate()?;

    println!("Successfully generated Python stub file!");
    Ok(())
}

#[cfg(not(feature = "stub-gen"))]
fn main() {
    eprintln!("The 'stub-gen' feature is not enabled. Enable it with `--features stub-gen`.");
    std::process::exit(1);
}
