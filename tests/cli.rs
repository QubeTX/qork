//! End-to-end CLI tests that don't touch the network.
//!
//! These exercise the argument surface and the offline guards (version, help,
//! the no-URL path, and the unquoted-spaces rejection). Live shortening is
//! verified manually against the API — see README "Development".

use assert_cmd::Command;
use predicates::prelude::*;

fn qork() -> Command {
    Command::cargo_bin("qork").expect("qork binary builds")
}

#[test]
fn version_prints_name_and_semver() {
    qork()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("qork "));
}

#[test]
fn help_describes_the_tool() {
    qork()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("shorten").and(predicate::str::contains("qork.me")));
}

#[test]
fn no_args_shows_help_and_exits_nonzero() {
    // arg_required_else_help: clap prints help and exits non-zero.
    qork().assert().failure();
}

#[test]
fn unquoted_spaces_are_rejected_offline() {
    // The whitespace guard fires before any HTTP call, so this is hermetic.
    qork()
        .arg("https://example.com/a b")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("quotes"));
}

#[test]
fn flag_without_url_is_a_usage_error() {
    // `--json` is an arg (so no auto-help), but there's no URL to shorten.
    qork().arg("--json").assert().failure().code(2);
}

#[test]
fn bare_help_prints_documentation() {
    // `qork help` must show help — NOT shorten the literal word "help".
    qork()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("shorten"));
}

#[test]
fn pasted_word_is_rejected_offline() {
    // A bare word ("asdf") becomes https://asdf — no dot, so the offline
    // structural check rejects it before any network call (hermetic).
    qork()
        .arg("asdf")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("doesn't look like"));
}
