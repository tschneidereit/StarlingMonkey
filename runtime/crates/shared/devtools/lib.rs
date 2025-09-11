/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! This module contains shared types and messages for use by devtools/script.
//! The traits are here instead of in script so that the devtools crate can be
//! modified independently of the rest of Servo.

#![crate_name = "devtools_traits"]
#![crate_type = "rlib"]
#![deny(unsafe_code)]

use core::fmt;
use std::collections::HashMap;
use std::fmt::Display;
use std::net::TcpStream;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitflags::bitflags;
// use http::{HeaderMap, Method};
use malloc_size_of_derive::MallocSizeOf;
// use net_traits::http_status::HttpStatus;
// use net_traits::request::Destination;
use serde::{Deserialize, Serialize};
use servo_url::ServoUrl;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum LogLevel {
    Log,
    Debug,
    Info,
    Warn,
    Error,
    Clear,
    Trace,
}

impl From<LogLevel> for log::Level {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Log => log::Level::Info,
            LogLevel::Clear => log::Level::Info,

            LogLevel::Debug => log::Level::Debug,
            LogLevel::Info => log::Level::Info,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Error => log::Level::Error,
            LogLevel::Trace => log::Level::Trace,
        }
    }
}

/// A console message as it is sent from script to the constellation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleMessage {
    pub log_level: LogLevel,
    pub filename: String,
    pub line_number: usize,
    pub column_number: usize,
    pub arguments: Vec<ConsoleMessageArgument>,
    pub stacktrace: Option<Vec<StackFrame>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ConsoleMessageArgument {
    String(String),
    Integer(i32),
    Number(f64),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StackFrame {
    pub filename: String,

    #[serde(rename = "functionName")]
    pub function_name: String,

    #[serde(rename = "columnNumber")]
    pub column_number: u32,

    #[serde(rename = "lineNumber")]
    pub line_number: u32,
}

bitflags! {
    #[derive(Deserialize, Serialize)]
    pub struct CachedConsoleMessageTypes: u8 {
        const PAGE_ERROR  = 1 << 0;
        const CONSOLE_API = 1 << 1;
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageError {
    #[serde(rename = "_type")]
    pub type_: String,
    pub error_message: String,
    pub source_name: String,
    pub line_text: String,
    pub line_number: u32,
    pub column_number: u32,
    pub category: String,
    pub time_stamp: u64,
    pub error: bool,
    pub warning: bool,
    pub exception: bool,
    pub strict: bool,
    pub private: bool,
}

/// Represents a console message as it is sent to the devtools
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConsoleLog {
    pub level: String,
    pub filename: String,
    pub line_number: u32,
    pub column_number: u32,
    pub time_stamp: u64,
    pub arguments: Vec<ConsoleArgument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stacktrace: Option<Vec<StackFrame>>,
}

impl From<ConsoleMessage> for ConsoleLog {
    fn from(value: ConsoleMessage) -> Self {
        let level = match value.log_level {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
            LogLevel::Clear => "clear",
            LogLevel::Trace => "trace",
            LogLevel::Log => "log",
        }
        .to_owned();

        let time_stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            level,
            filename: value.filename,
            line_number: value.line_number as u32,
            column_number: value.column_number as u32,
            time_stamp,
            arguments: value.arguments.into_iter().map(|arg| arg.into()).collect(),
            stacktrace: value.stacktrace,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum CachedConsoleMessage {
    PageError(PageError),
    ConsoleLog(ConsoleLog),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ConsoleArgument {
    String(String),
    Integer(i32),
    Number(f64),
}

impl From<ConsoleMessageArgument> for ConsoleArgument {
    fn from(value: ConsoleMessageArgument) -> Self {
        match value {
            ConsoleMessageArgument::String(string) => Self::String(string),
            ConsoleMessageArgument::Integer(integer) => Self::Integer(integer),
            ConsoleMessageArgument::Number(number) => Self::Number(number),
        }
    }
}

impl From<String> for ConsoleMessageArgument {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

pub struct ConsoleMessageBuilder {
    level: LogLevel,
    filename: String,
    line_number: u32,
    column_number: u32,
    arguments: Vec<ConsoleMessageArgument>,
    stack_trace: Option<Vec<StackFrame>>,
}

impl ConsoleMessageBuilder {
    pub fn new(level: LogLevel, filename: String, line_number: u32, column_number: u32) -> Self {
        Self {
            level,
            filename,
            line_number,
            column_number,
            arguments: vec![],
            stack_trace: None,
        }
    }

    pub fn attach_stack_trace(&mut self, stack_trace: Vec<StackFrame>) -> &mut Self {
        self.stack_trace = Some(stack_trace);
        self
    }

    pub fn add_argument(&mut self, argument: ConsoleMessageArgument) -> &mut Self {
        self.arguments.push(argument);
        self
    }

    pub fn finish(self) -> ConsoleMessage {
        ConsoleMessage {
            log_level: self.level,
            filename: self.filename,
            line_number: self.line_number as usize,
            column_number: self.column_number as usize,
            arguments: self.arguments,
            stacktrace: self.stack_trace,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ShadowRootMode {
    Open,
    Closed,
}

impl fmt::Display for ShadowRootMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Closed => write!(f, "close"),
        }
    }
}
