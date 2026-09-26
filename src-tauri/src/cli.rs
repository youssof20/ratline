//! CLI flags shared by main and the Tauri setup path.

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct LaunchArgs {
    pub version: bool,
    pub demo: Option<DemoKind>,
    pub demo_role: DemoRole,
    pub demo_code: Option<String>,
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoKind {
    Conversation,
    Commands,
}

impl DemoKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DemoKind::Conversation => "conversation",
            DemoKind::Commands => "commands",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "conversation" | "1" | "chat" => Some(DemoKind::Conversation),
            "commands" | "2" | "cmd" => Some(DemoKind::Commands),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DemoRole {
    #[default]
    Host,
    Joiner,
}

impl DemoRole {
    pub fn as_str(self) -> &'static str {
        match self {
            DemoRole::Host => "host",
            DemoRole::Joiner => "joiner",
        }
    }
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> LaunchArgs {
    let mut out = LaunchArgs::default();
    let mut iter = args.into_iter().skip(1); // skip exe
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--version" | "-V" => out.version = true,
            "--demo" => {
                if let Some(v) = iter.next() {
                    out.demo = DemoKind::parse(&v);
                }
            }
            "--demo-role" => {
                if let Some(v) = iter.next() {
                    out.demo_role = match v.as_str() {
                        "joiner" | "peer" | "b" => DemoRole::Joiner,
                        _ => DemoRole::Host,
                    };
                }
            }
            "--demo-code" => {
                out.demo_code = iter.next();
            }
            "--data-dir" => {
                out.data_dir = iter.next().map(PathBuf::from);
            }
            _ => {}
        }
    }
    // env fallbacks for spawned peers
    if out.demo.is_none() {
        if let Ok(v) = std::env::var("RATLINE_DEMO") {
            out.demo = DemoKind::parse(&v);
        }
    }
    if out.demo_code.is_none() {
        out.demo_code = std::env::var("RATLINE_DEMO_CODE").ok();
    }
    if matches!(out.demo_role, DemoRole::Host) {
        if std::env::var("RATLINE_DEMO_ROLE").ok().as_deref() == Some("joiner") {
            out.demo_role = DemoRole::Joiner;
        }
    }
    if out.data_dir.is_none() {
        out.data_dir = std::env::var("RATLINE_DATA_DIR").ok().map(PathBuf::from);
    }
    out
}

#[cfg(windows)]
pub fn ensure_console() {
    // Attach to parent console so `ratline --version` prints in a terminal.
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
        fn AllocConsole() -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            let _ = AllocConsole();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_demo_and_version() {
        let a = parse_args(
            ["ratline", "--version", "--demo", "commands"]
                .into_iter()
                .map(String::from),
        );
        assert!(a.version);
        assert_eq!(a.demo, Some(DemoKind::Commands));
    }

    #[test]
    fn parses_joiner() {
        let a = parse_args(
            [
                "ratline",
                "--demo",
                "conversation",
                "--demo-role",
                "joiner",
                "--demo-code",
                "P-TEST",
            ]
            .into_iter()
            .map(String::from),
        );
        assert_eq!(a.demo_role, DemoRole::Joiner);
        assert_eq!(a.demo_code.as_deref(), Some("P-TEST"));
    }
}
