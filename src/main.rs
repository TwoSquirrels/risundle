use clap::Parser;

/// A C++ bundler with tree-shaking for competitive programming
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input file path
    #[arg(short, long)]
    input: Option<String>,

    /// Output file path
    #[arg(short, long)]
    output: Option<String>,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.verbose {
        println!("Running risundle...");
    }

    match (args.input, args.output) {
        (Some(input), Some(output)) => {
            println!("Input: {}", input);
            println!("Output: {}", output);
            // TODO: Implement bundling logic
            Ok(())
        }
        (Some(input), None) => {
            println!("Input: {}", input);
            println!("Output: <stdout>");
            // TODO: Implement bundling logic with stdout
            Ok(())
        }
        (None, _) => {
            anyhow::bail!("No input file specified. Use --help for usage information.")
        }
    }
}
