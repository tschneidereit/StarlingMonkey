use base::id::{PipelineId, PipelineNamespace, PipelineNamespaceId};
use clap::Parser;
use constellation_traits::{ScriptToConstellationChan, WorkerGlobalScopeInit};
use crossbeam_channel::{unbounded, Receiver};
use devtools_traits::WorkerId;
use embedder_traits::{EmbedderMsg, EmbedderProxy, EventLoopWaker, ScriptToEmbedderChan};
use net::protocols::ProtocolRegistry;
use net::resource_thread::new_resource_threads;
use net_traits::init_fetch_channel;
use profile_traits::generic_channel;
use script::{CanGc, GlobalScope};
use std::fs;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use storage_traits::StorageThreads;
use uuid::Uuid;

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
    let pipeline_id = PipelineId::new();

    init_fetch_channel();

    let url = servo_url::ServoUrl::parse("http://evalcode").unwrap();
    let time_profiler_chan = profile::time::Profiler::create(
        &None, // &opts.time_profiling,
        None, //opts.time_profiler_trace_path.clone(),
    );
    let mem_profiler_chan = profile::mem::Profiler::create();

    let (constellation_sender, _constellation_receiver) =
        generic_channel::channel(time_profiler_chan.clone()).unwrap();
    let script_to_constellation_chan = ScriptToConstellationChan {
        sender: constellation_sender,
        pipeline_id,
    };

    let event_loop_waker: Box<dyn EventLoopWaker> = Box::new(DefaultEventLoopWaker);
    let (embedder_proxy, embedder_receiver) = create_embedder_channel(event_loop_waker.clone());
    let embedder_chan = embedder_proxy.sender.clone();
    let eventloop_waker = event_loop_waker.clone();
    let script_to_embedder_chan = ScriptToEmbedderChan::new(embedder_chan, eventloop_waker);
    let (storage_sender, storage_receiver) =
        generic_channel::channel(time_profiler_chan.clone()).unwrap();
    let storage_threads: StorageThreads = StorageThreads::new(storage_sender);

    let protocols = ProtocolRegistry::with_internal_protocols();

    let (public_resource_threads, private_resource_threads, _async_runtime) = new_resource_threads(
        None,
        time_profiler_chan.clone(),
        mem_profiler_chan.clone(),
        embedder_proxy.clone(),
        None,
        None,
        true,
        Arc::new(protocols),
    );

    let init = WorkerGlobalScopeInit {
        pipeline_id,
        // devtools_chan,
        origin: url.origin(),
        creation_url: url,
        mem_profiler_chan,
        time_profiler_chan,
        to_devtools_sender: None,
        from_devtools_sender: None,
        script_to_constellation_chan,
        script_to_embedder_chan,
        resource_threads: private_resource_threads,
        storage_threads,
        worker_id: WorkerId(Uuid::default()),
        inherited_secure_context: None,
    };
    let global = GlobalScope::run_worker_scope(
        init,
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

    // Keep processing events while there are pending timers or other async activities
    while global.has_pending_activity() {
        // Process embedder messages
        while let Ok(msg) = embedder_receiver.try_recv() {
            handle_embedder_msg(msg);
        }

        // Wait a bit for the next timer/event to be ready
        sleep(Duration::from_millis(10));
        global.process_events(CanGc::note());
    }

    exit(0);
}

struct DefaultEventLoopWaker;

impl EventLoopWaker for DefaultEventLoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(DefaultEventLoopWaker)
    }
}

fn handle_embedder_msg(msg: EmbedderMsg) {
    use embedder_traits::WebResourceResponseMsg;

    match msg {
        EmbedderMsg::WebResourceRequested(_webview_id, _request, sender) => {
            // For now, we don't intercept any requests - just let them proceed normally
            if let Err(e) = sender.send(WebResourceResponseMsg::DoNotIntercept) {
                eprintln!("[DEBUG] Failed to send DoNotIntercept response: {:?}", e);
            }
        }
        _ => {
            // Ignore other embedder messages for now
            eprintln!("[DEBUG] Received unhandled embedder message");
        }
    }
}

fn create_embedder_channel(
    event_loop_waker: Box<dyn EventLoopWaker>,
) -> (EmbedderProxy, Receiver<EmbedderMsg>) {
    let (sender, receiver) = unbounded();
    (
        EmbedderProxy {
            sender,
            event_loop_waker,
        },
        receiver,
    )
}
