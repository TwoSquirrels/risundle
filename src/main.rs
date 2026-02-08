use clap::Parser;

/// A C++ bundler with tree-shaking for competitive programming
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input file path
    #[arg(short, long)]
    input: String,

    /// Output file path
    #[arg(short, long)]
    output: Option<String>,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let output = args.output.as_deref().unwrap_or("<stdout>");

    if args.verbose {
        println!("Running risundle...");
        println!("Input: {}", args.input);
        println!("Output: {}", output);
    } else {
        println!("Input: {}", args.input);
        println!("Output: {}", output);
    }

    // TODO: Implement bundling logic
    Ok(())
}
