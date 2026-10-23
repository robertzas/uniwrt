//! `uniwrt version`: build information as text or JSON.

use std::io::{self, Write};

use crate::{build_info::BuildInfo, cli::VersionArgs};

pub(crate) fn run(args: &VersionArgs) -> anyhow::Result<()> {
    write(&BuildInfo::current(), args.json, &mut io::stdout().lock())
}

fn write(info: &BuildInfo, json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    if json {
        serde_json::to_writer(&mut *out, info)?;
        writeln!(out)?;
    } else {
        write!(out, "{info}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BuildInfo, write};
    use crate::build_info;

    #[test]
    fn text_starts_with_name_and_version() {
        let mut out = Vec::new();
        write(&BuildInfo::current(), false, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.starts_with(&format!("uniwrt {}\n", build_info::VERSION)),
            "{text}"
        );
        assert!(text.contains(build_info::TARGET));
    }

    #[test]
    fn json_has_every_field() {
        let mut out = Vec::new();
        write(&BuildInfo::current(), true, &mut out).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["version"], build_info::VERSION);
        assert_eq!(json["commit"], build_info::COMMIT);
        assert_eq!(json["built_at"], build_info::BUILT_AT);
        assert_eq!(json["target"], build_info::TARGET);
        assert!(json["ui"] == "embedded" || json["ui"] == "placeholder");
    }
}
