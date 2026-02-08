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

    if args.verbose {
        println!("Running risundle...");
    }

    match args.output {
        Some(output) => {
            println!("Input: {}", args.input);
            println!("Output: {}", output);
            // TODO: Implement bundling logic
            Ok(())
        }
        None => {
            println!("Input: {}", args.input);
            println!("Output: <stdout>");
            // TODO: Implement bundling logic with stdout
            Ok(())
        }
    }
}
