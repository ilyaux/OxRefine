//! `ox --help` and `ox --version` behave like other command-line tools: stdout, exit 0.
use std::process::Command;

fn ox(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ox")).args(args).output().unwrap()
}

#[test]
fn version_and_help_print_to_stdout_and_succeed() {
    for flag in ["--version", "-V", "version"] {
        let out = ox(&[flag]);
        assert!(out.status.success(), "{flag}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("ox {}", env!("CARGO_PKG_VERSION")));
    }
    for args in [&["--help"][..], &["-h"], &["help"], &["serve", "--help"], &["apply", "-h"]] {
        let out = ox(args);
        assert!(out.status.success(), "{args:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("usage:") && text.contains("ox serve") && text.contains("ox --version"), "{args:?}: {text}");
    }
    // no arguments is still a usage error
    let out = ox(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));
}
