//! CLI argument parsing for the StarlingMonkey runtime.
//!
//! Replaces the C++ `starling::ConfigParser` from `include/config-parser.h`.
//! Uses clap's derive API for type-safe argument parsing.

use clap::Parser;

/// Runtime configuration, parsed from CLI args or env var.
///
/// This is the Rust equivalent of `api::EngineConfig` from C++.
#[derive(Parser, Debug, Clone)]
#[command(name = "starling", about = "StarlingMonkey JS runtime")]
pub struct EngineConfig {
    /// Path to the content script to execute.
    #[arg(default_value = "./index.js")]
    pub script_path: String,

    /// Evaluate inline script instead of a file.
    #[arg(short = 'e', long = "eval")]
    pub eval_script: Option<String>,

    /// Path to an initialization script (runs in a separate global before content).
    #[arg(short = 'i', long = "initializer-script")]
    pub initializer_script_path: Option<String>,

    /// Enable verbose logging.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Enable script debugging via socket connection.
    #[arg(short = 'd', long = "debug")]
    pub debugging: bool,

    /// Use classic (non-module) script mode.
    #[arg(long = "legacy-script")]
    pub legacy_script: bool,

    /// Enable WPT (Web Platform Tests) mode.
    #[arg(long = "wpt-mode")]
    pub wpt_mode: bool,

    /// Override the location URL for initialization.
    #[arg(long = "init-location")]
    pub init_location: Option<String>,

    /// Strip this prefix from script paths.
    #[arg(long = "strip-path-prefix")]
    pub path_prefix: Option<String>,

    /// Pre-initialize the runtime (used during wizer snapshot).
    #[arg(skip)]
    pub pre_initialize: bool,
}

impl EngineConfig {
    /// Whether to use ES module mode (the default, unless --legacy-script).
    pub fn module_mode(&self) -> bool {
        !self.legacy_script
    }

    /// The effective content script source — either from --eval or from the file path.
    pub fn content_script(&self) -> Option<&str> {
        self.eval_script.as_deref()
    }

    /// Parse from an argument string (e.g., from STARLINGMONKEY_CONFIG env var).
    ///
    /// Splits the string on whitespace, respecting single/double quotes.
    pub fn from_arg_string(args: &str) -> Result<Self, clap::Error> {
        let argv = split_args(args);
        // Prepend program name for clap
        let mut full_argv = vec!["starling".to_string()];
        full_argv.extend(argv);
        Self::try_parse_from(full_argv)
    }

    /// Parse from WASI CLI arguments (as provided by the host).
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self, clap::Error> {
        Self::try_parse_from(args)
    }

    /// Parse from the STARLINGMONKEY_CONFIG environment variable.
    pub fn from_env() -> Result<Self, clap::Error> {
        match std::env::var("STARLINGMONKEY_CONFIG") {
            Ok(config) => Self::from_arg_string(&config),
            Err(_) => Self::try_parse_from(["starling"]),
        }
    }

    /// Parse from stdin (for wizer pre-initialization).
    /// Reads a single line of arguments from stdin.
    pub fn from_stdin() -> Result<Self, clap::Error> {
        let mut input = String::new();
        if std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).is_err() {
            return Self::try_parse_from(["starling"]);
        }
        let mut config = Self::from_arg_string(input.trim())?;
        config.pre_initialize = true;
        Ok(config)
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::try_parse_from(["starling"]).unwrap()
    }
}

/// Split an argument string into individual arguments, respecting quotes.
fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in s.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if !in_single_quote => {
                escape_next = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' | '\n' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EngineConfig::default();
        assert_eq!(config.script_path, "./index.js");
        assert!(config.module_mode());
        assert!(!config.verbose);
        assert!(!config.debugging);
        assert!(!config.wpt_mode);
    }

    #[test]
    fn test_parse_args() {
        let config = EngineConfig::from_arg_string("-v --legacy-script app.js").unwrap();
        assert_eq!(config.script_path, "app.js");
        assert!(config.verbose);
        assert!(!config.module_mode());
    }

    #[test]
    fn test_split_args_quotes() {
        let args = split_args(r#"--eval "console.log('hello world')" -v"#);
        assert_eq!(args, vec!["--eval", "console.log('hello world')", "-v"]);
    }

    #[test]
    fn test_split_args_single_quotes() {
        let args = split_args("--eval 'console.log(42)' -v");
        assert_eq!(args, vec!["--eval", "console.log(42)", "-v"]);
    }

    #[test]
    fn test_split_args_escape() {
        let args = split_args(r"--eval hello\ world");
        assert_eq!(args, vec!["--eval", "hello world"]);
    }
}
