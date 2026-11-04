use std::{fs, io::Write as _, path::Path, process::Command as Process, time::Duration};

use super::{
    Checked, Command, Done, FILES, Finished, Footprint, Open, OpenState, Outcome, Reason, Status,
    TxnId, apply, code, put, stage, status, uninstall,
};
use crate::{Error, ExecOutput, ExitStatus};

fn output(code: u32, stdout: &str, stderr: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// Runs a command's argv and stdin on the host.
fn run_here<T: serde::de::DeserializeOwned>(command: &Command<T>) -> crate::Result<T> {
    let argv = &command.request.argv;
    let out = Process::new(&argv[0]).args(&argv[1..]).output().unwrap();
    command.parse(output(
        u32::try_from(out.status.code().unwrap()).unwrap(),
        std::str::from_utf8(&out.stdout).unwrap(),
        std::str::from_utf8(&out.stderr).unwrap(),
    ))
}

fn id() -> TxnId {
    TxnId::new("j42").unwrap()
}

#[test]
fn ids_and_names_are_what_the_tool_takes() {
    assert!(TxnId::new("j42-node_7").is_ok());
    for bad in ["", "a b", "a;b", "ä", &"x".repeat(65)] {
        assert!(TxnId::new(bad).is_err(), "{bad:?}");
    }
    assert!(stage(&id(), [("dnsmasq", true), ("odhcpd", false)]).is_ok());
    assert!(stage(&id(), [("dns masq", true)]).is_err());
    assert!(put(&id(), "../network", "x").is_err());
}

#[test]
fn commands_are_argv_arrays() {
    let staged = stage(&id(), [("dnsmasq", false), ("firewall", true)]).unwrap();
    assert_eq!(
        staged.request.argv,
        [
            "/etc/uniwrt/txn",
            "stage",
            "j42",
            "dnsmasq=off",
            "firewall=on"
        ]
    );
    let put = put(&id(), "network", "config interface 'lan'\n").unwrap();
    assert_eq!(
        put.request.argv,
        ["/etc/uniwrt/txn", "put", "j42", "network"]
    );
    assert_eq!(
        put.request.stdin.as_deref(),
        Some(&b"config interface 'lan'\n"[..])
    );
    let applied = apply(&id(), Duration::from_secs(120));
    assert_eq!(
        applied.request.argv,
        ["/etc/uniwrt/txn", "apply", "j42", "120"]
    );
}

#[test]
fn answers_parse_and_errors_carry_their_code() {
    let checked = super::check(&id());
    assert_eq!(
        checked
            .parse(output(
                0,
                "{\"ok\":{\"packages\":[\"network\",\"system\"]}}\n",
                ""
            ))
            .unwrap(),
        Checked {
            packages: vec!["network".to_owned(), "system".to_owned()]
        }
    );
    let err = checked
        .parse(output(
            1,
            "{\"err\":{\"code\":\"invalid\",\"msg\":\"package network: uci: Parse error\"}}\n",
            "",
        ))
        .unwrap_err();
    let Error::Txn { command, code, msg } = err else {
        panic!("{err:?}")
    };
    assert_eq!((command.as_str(), code.as_str()), ("check", code::INVALID));
    assert!(msg.contains("Parse error"));
    // Without an answer, the exit status and stderr say what happened.
    let err = checked
        .parse(output(127, "", "sh: /etc/uniwrt/txn: not found"))
        .unwrap_err();
    assert!(matches!(err, Error::Failed { .. }), "{err:?}");
    let err = checked.parse(output(0, "garbage\n", "")).unwrap_err();
    assert!(matches!(err, Error::Protocol(_)), "{err:?}");
}

#[test]
fn the_status_reads_open_and_finished_transactions() {
    let command = status();
    let answer = r#"{"ok":{"version":"ab12","open":{"id":"j7","state":"reloading","remaining_s":118},"last":{"id":"j6","outcome":"rolled_back","reason":"timeout","at":1762000000}}}"#;
    assert_eq!(
        command.parse(output(0, answer, "")).unwrap(),
        Some(Status {
            version: "ab12".to_owned(),
            open: Some(Open {
                id: "j7".to_owned(),
                state: OpenState::Reloading,
                remaining_s: 118,
            }),
            last: Some(Finished {
                id: "j6".to_owned(),
                outcome: Outcome::RolledBack,
                reason: Some(Reason::Timeout),
                at: 1_762_000_000,
            }),
        })
    );
}

/// The host has no footprint: the scripts answer as a node without one does.
#[test]
fn without_a_footprint_status_is_none_and_uninstall_is_done() {
    assert!(!Path::new(super::TOOL).exists());
    assert_eq!(run_here(&status()).unwrap(), None);
    assert_eq!(run_here(&uninstall()).unwrap(), Done {});
}

/// Every file in `node-agent/txn/` is in the footprint, and the archive extracts and checks
/// with the host's tar and sha256sum, as the install script does on the node.
#[test]
fn the_footprint_holds_every_file_and_verifies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node-agent/txn");
    let mut found: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    let embedded: Vec<_> = FILES.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(embedded, found);

    let footprint = Footprint::get();
    let dir = tempfile::tempdir().unwrap();
    let mut tar = Process::new("tar")
        .args(["xf", "-"])
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    tar.stdin
        .take()
        .unwrap()
        .write_all(footprint.install().request.stdin.as_deref().unwrap())
        .unwrap();
    assert!(tar.wait().unwrap().success());
    let check = Process::new("sha256sum")
        .args(["-c", "--quiet", "SHA256SUMS"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(check.success());
    let version = fs::read_to_string(dir.path().join("VERSION")).unwrap();
    assert_eq!(version.trim(), footprint.version());
    // The scripts parse as POSIX sh.
    for name in ["txn", "uniwrt-guard"] {
        let syntax = Process::new("sh")
            .args(["-n", name])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(syntax.success(), "{name}");
    }
}
