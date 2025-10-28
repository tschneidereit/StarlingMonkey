use base::id::{PipelineId, PipelineNamespace, PipelineNamespaceId};
use clap::Parser;
use script::{CanGc, GlobalScope};
use servo_url::MutableOrigin;
use std::fs;
use std::path::PathBuf;
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "starlingshell")]
#[command(about = "A JavaScript runtime using Servo's WebIDL bindings", long_about = None)]
struct Args {
    /// Path to the JavaScript file to execute
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Execute inline JavaScript code
    #[arg(short = 'e', long = "eval", value_name = "CODE")]
    eval: Option<String>,
}

fn main() {
    let args = Args::parse();

    let mut builder = env_logger::Builder::from_default_env();
    builder.init();

    let _init = script::init();
    PipelineNamespace::install(PipelineNamespaceId(0));
    let url = servo_url::ServoUrl::parse("http://evalcode").unwrap();
    let origin = MutableOrigin::new(url.origin());
    let global = GlobalScope::run_worker_scope(
        PipelineId::new(),
        origin,
        url.clone(),
        url,
        "content".to_string(),
    );

    // Determine the script to execute
    let script = match (args.file, args.eval) {
        (Some(file_path), None) => {
            // Load script from file
            fs::read_to_string(&file_path).unwrap_or_else(|err| {
                eprintln!("Error reading file '{}': {}", file_path.display(), err);
                exit(1);
            })
        }
        (None, Some(code)) => {
            // Execute inline code
            code
        }
        (None, None) => {
            eprintln!(
                "Error: Please provide either a file to execute or use -e/--eval for inline code"
            );
            exit(1);
        }
        (Some(_), Some(_)) => {
            eprintln!("Error: Cannot specify both a file and inline code");
            exit(1);
        }
    };

    global.execute_script(&script, CanGc::note());
    global.process_events(CanGc::note());
    sleep(Duration::from_millis(50));
    global.process_events(CanGc::note());
    exit(0);
}
