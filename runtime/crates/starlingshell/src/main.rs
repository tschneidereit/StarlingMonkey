use std::process::exit;
use script::{CanGc, GlobalScope};
use servo_url::MutableOrigin;

fn main() {
    let mut builder = env_logger::Builder::from_default_env();
    builder.init();

    let _init = script::init();
    let url = servo_url::ServoUrl::parse("http://evalcode").unwrap();
    let origin = MutableOrigin::new(url.origin());
    let global = GlobalScope::run_worker_scope(origin, url.clone(), url, "content".to_string());
    global.execute_script("console.log('Hello from within a global created in Rust with Servo\\'s WebIDL bindings, using a WebIDL based Console!');".into(), CanGc::note());
    exit(0);
}
