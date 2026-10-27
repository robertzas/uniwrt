//! Argv to a command line for the node's shell. SSH carries one string, which the remote user's
//! shell (`ash` on OpenWrt) splits, so every argument is quoted and values are never interpolated
//! into shell syntax (AGENTS.md, Node side).

use crate::{Error, Result};

/// Joins `argv` into a command line that `sh -c` splits back into exactly these arguments.
/// Arguments made only of safe characters stay readable; anything else is single-quoted. A
/// reserved word in command position is quoted too, so it runs as a command.
pub fn command_line<S: AsRef<str>>(argv: &[S]) -> Result<String> {
    if argv.is_empty() {
        return Err(Error::InvalidArgv("argv is empty"));
    }
    let mut line = String::with_capacity(argv.iter().map(|arg| arg.as_ref().len() + 3).sum());
    for (index, arg) in argv.iter().enumerate() {
        let arg = arg.as_ref();
        if arg.contains('\0') {
            return Err(Error::InvalidArgv("an argument contains a NUL byte"));
        }
        if index > 0 {
            line.push(' ');
        }
        let plain = !arg.is_empty()
            && arg.bytes().all(is_plain)
            && !(index == 0 && RESERVED.contains(&arg));
        if plain {
            line.push_str(arg);
        } else {
            line.push('\'');
            line.push_str(&arg.replace('\'', r"'\''"));
            line.push('\'');
        }
    }
    Ok(line)
}

/// Bytes with no meaning to a POSIX shell anywhere in a word. `=` is left out (it would turn a
/// first word into an assignment), and so is `~` (tilde expansion).
fn is_plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_-./:@%+,".contains(&byte)
}

/// POSIX reserved words, plus `function` and `select` (ash and bash know them).
const RESERVED: &[&str] = &[
    "!", "{", "}", "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if",
    "in", "select", "then", "until", "while",
];

#[cfg(test)]
mod tests {
    use std::process::Command;

    use proptest::prelude::*;

    use super::command_line;

    #[test]
    fn plain_words_stay_readable_and_the_rest_is_quoted() {
        let line = command_line(&["ubus", "call", "network.interface.wan", "status"]).unwrap();
        assert_eq!(line, "ubus call network.interface.wan status");
        let line = command_line(&["echo", "it's", "", "$HOME", "a b", "x=1", "~"]).unwrap();
        assert_eq!(line, r"echo 'it'\''s' '' '$HOME' 'a b' 'x=1' '~'");
        assert_eq!(command_line(&["if", "if"]).unwrap(), "'if' if");
    }

    #[test]
    fn empty_argv_and_nul_bytes_are_refused() {
        assert!(command_line::<&str>(&[]).is_err());
        assert!(command_line(&["echo", "a\0b"]).is_err());
    }

    /// Runs the quoted line with the local `sh`, which prints each argument it received.
    fn split_by_sh(words: &[String]) -> Vec<String> {
        let mut argv = vec!["printf".to_owned(), r"%s\0".to_owned()];
        argv.extend_from_slice(words);
        let output = Command::new("sh")
            .arg("-c")
            .arg(command_line(&argv).unwrap())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let mut split: Vec<String> = stdout.split('\0').map(str::to_owned).collect();
        assert_eq!(split.pop().as_deref(), Some(""));
        split
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn sh_splits_the_line_back_into_the_same_arguments(
            args in prop::collection::vec("[^\0]{0,12}", 1..6),
        ) {
            prop_assert_eq!(split_by_sh(&args), args);
        }

        #[test]
        fn shell_syntax_survives_as_literal_text(
            args in prop::collection::vec(r#"[ -/:-@\[-`{-~\t\n'"\\$*?]{1,8}"#, 1..6),
        ) {
            prop_assert_eq!(split_by_sh(&args), args);
        }
    }
}
