//! Package commands through the controller's feed mirror (features/lifecycle.md, Offline
//! packages, *Per command*; D-147). Every package manager command uniwrt runs on a node goes
//! through [`Via::wrap`] while the mirror is on: one fixed script ([`SCRIPT`]) that points the
//! node's package manager at the controller for that command only, so the node's own feed
//! configuration never changes.
//!
//! The script takes the controller's address from the login (`SSH_CONNECTION`'s client side:
//! the controller as the node sees it) and its HTTPS port from the arguments. It reads the
//! node's feeds as the package manager would (opkg: `/etc/opkg.conf`, then `/etc/opkg/*.conf`,
//! the first `src` of a name winning; apk: `/etc/apk/repositories` and
//! `repositories.d/*.list`), asks the controller for each feed's index at
//! `https://<controller>:<port>/feeds/<host>/<path>/…` (all at once, 5 s each), and uses the
//! controller for the feeds it answers. A feed it doesn't serve keeps its own URL; when it serves
//! none (unreachable, the mirror off, the feeds not planned yet), the command runs as it is.
//!
//! The command then runs with a shim of the package manager first in `PATH`, so a script that
//! calls it (the wpad swap's `opkg download`) is covered too. apk's shim adds
//! `--repositories-file` with every line, mirrored or not; opkg's adds `-f` with the mirrored
//! `src` lines first and then `/etc/opkg.conf` (opkg still reads `/etc/opkg/*.conf` after
//! it, and the first declaration of a name wins, so it warns of the duplicates and skips them).
//! Both add `--no-check-certificate`: the controller's certificate isn't one the node trusts,
//! and the feeds' signatures, which the node checks as ever, carry the integrity.
//!
//! The shim and its configuration live in a temporary directory removed when the command ends,
//! or, with [`Via::wrap_in`], in `<dir>/.mirror`, kept and reused by every later command
//! wrapped in the same `dir`: apk keeps its indexes under each repository URL, so the wpad
//! swap, which installs offline from a cache, must name the same URLs as its fetch did.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// The script's name in `$0`, which tells its argv apart.
pub const NAME: &str = "uniwrt-pkg";

/// The directory below a [`Via::wrap_in`] directory that keeps the shim.
pub const KEPT: &str = ".mirror";

/// `$1` the controller's HTTPS port, `$2` the directory to keep the shim in (empty: a
/// temporary one), then the command.
pub const SCRIPT: &str = r#"port=$1 keep=$2
shift 2
c=${SSH_CONNECTION%% *}
case $c in
'' | *[!0-9A-Fa-f.:]*) exec "$@" ;;
*:*) c="[$c]" ;;
esac
if command -v apk >/dev/null 2>&1; then
	m=apk
elif command -v opkg >/dev/null 2>&1; then
	m=opkg
else
	exec "$@"
fi
real=$(command -v "$m")
if [ -n "$keep" ]; then
	d=$keep/.mirror
else
	d=$(mktemp -d /var/run/uniwrt-pkg.XXXXXX) || exec "$@"
	trap 'rm -rf "$d"' EXIT
	trap 'exit 129' HUP
	trap 'exit 130' INT
	trap 'exit 143' TERM
fi
if [ ! -x "$d/bin/$m" ]; then
	rm -rf "$d"
	mkdir -p "$d/bin" || exec "$@"
	b=https://$c:$port/feeds
	if [ "$m" = apk ]; then
		cat /etc/apk/repositories /etc/apk/repositories.d/*.list 2>/dev/null | awk -v b="$b" '
/^[ \t]*(#|$)/ { next }
$NF ~ /^https?:\/\/[^\/]+\/.+\/packages\.adb$/ {
	u = $NF; sub(/^https?:\/\//, "", u); h = u; sub(/\/.*/, "", h)
	v = b "/" tolower(h) substr(u, length(h) + 1)
	print NR "\t" v "\t" (NF > 1 ? $1 " " : "") v "\t" $0
	next
}
{ print NR "\t-\t\t" $0 }'
	else
		cat /etc/opkg.conf /etc/opkg/*.conf 2>/dev/null | awk -v b="$b" '
($1 == "src" || $1 == "src/gz") && NF >= 3 && !seen[$2]++ && $3 ~ /^https?:\/\/[^\/]+\/./ {
	u = $3; sub(/^https?:\/\//, "", u); sub(/\/+$/, "", u); h = u; sub(/\/.*/, "", h)
	v = b "/" tolower(h) substr(u, length(h) + 1)
	print NR "\t" v "/Packages.gz\t" $1 " " $2 " " v "\t"
}'
	fi >"$d/feeds"
	t=$(printf '\t')
	while IFS=$t read -r i p l o; do
		[ "$p" = - ] || {
			wget -q -s -T 5 --no-check-certificate "$p" && : >"$d/ok.$i"
		} </dev/null >/dev/null 2>&1 &
	done <"$d/feeds"
	wait
	n=0
	while IFS=$t read -r i p l o; do
		if [ -f "$d/ok.$i" ]; then
			echo "$l"
			n=$((n + 1))
		elif [ -n "$o" ]; then
			echo "$o"
		fi
	done <"$d/feeds" >"$d/conf"
	if [ "$n" = 0 ]; then
		rm -rf "$d"
		"$@"
		exit
	fi
	if [ "$m" = apk ]; then
		printf '#!/bin/sh\nexec %s --repositories-file %s/conf --no-check-certificate "$@"\n' "$real" "$d"
	else
		cat /etc/opkg.conf >>"$d/conf"
		printf '#!/bin/sh\nexec %s -f %s/conf --no-check-certificate "$@"\n' "$real" "$d"
	fi >"$d/bin/$m"
	chmod +x "$d/bin/$m" || exit 1
fi
PATH=$d/bin:$PATH "$@""#;

/// Whether, and through which port, nodes fetch packages through the controller. Clones share
/// it; the default never does.
#[derive(Debug, Clone, Default)]
pub struct Via {
    inner: Option<Arc<Inner>>,
}

#[derive(Debug)]
struct Inner {
    port: u16,
    on: AtomicBool,
}

impl Via {
    /// Through the controller's HTTPS listener on `port`, once [`Via::set`] turns it on.
    pub fn new(port: u16) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                port,
                on: AtomicBool::new(false),
            })),
        }
    }

    /// Turns it on or off: the mirror's `enabled` setting, while the mirror runs.
    pub fn set(&self, on: bool) {
        if let Some(inner) = &self.inner {
            inner.on.store(on, Ordering::Relaxed);
        }
    }

    /// The port nodes fetch through, while it is on.
    pub fn port(&self) -> Option<u16> {
        self.inner
            .as_ref()
            .filter(|inner| inner.on.load(Ordering::Relaxed))
            .map(|inner| inner.port)
    }

    /// `argv`, a package manager command (or a script that runs one), through the controller
    /// while it is on; else `argv` itself.
    pub fn wrap(&self, argv: Vec<String>) -> Vec<String> {
        self.wrap_in("", argv)
    }

    /// As [`Via::wrap`], the shim kept in `<dir>/.mirror` and reused by the next command
    /// wrapped in `dir`.
    pub fn wrap_in(&self, dir: &str, argv: Vec<String>) -> Vec<String> {
        let Some(port) = self.port() else {
            return argv;
        };
        ["sh", "-c", SCRIPT, NAME, &port.to_string(), dir]
            .into_iter()
            .map(str::to_owned)
            .chain(argv)
            .collect()
    }
}

/// The command inside a wrapped `argv`, with the port and directory it was wrapped with; `None`
/// for a command that isn't (the simulator answers the command inside).
pub fn unwrap(argv: &[String]) -> Option<(u16, &str, &[String])> {
    match argv {
        [sh, c, script, name, port, dir, rest @ ..]
            if sh == "sh" && c == "-c" && script == SCRIPT && name == NAME =>
        {
            Some((port.parse().ok()?, dir, rest))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Via, unwrap};

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|&word| word.to_owned()).collect()
    }

    #[test]
    fn a_command_goes_through_the_controller_only_while_on() {
        let update = argv(&["apk", "update"]);
        assert_eq!(Via::default().wrap(update.clone()), update);
        let via = Via::new(8443);
        assert_eq!(via.wrap(update.clone()), update);
        assert_eq!(via.port(), None);

        via.clone().set(true);
        assert_eq!(via.port(), Some(8443));
        let wrapped = via.wrap(update.clone());
        assert_eq!(wrapped[..2], ["sh", "-c"]);
        assert_eq!(unwrap(&wrapped), Some((8443, "", update.as_slice())));
        let kept = via.wrap_in("/var/run/uniwrt-wpad", update.clone());
        assert_eq!(
            unwrap(&kept),
            Some((8443, "/var/run/uniwrt-wpad", update.as_slice()))
        );

        via.set(false);
        assert_eq!(via.wrap(update.clone()), update);
        assert_eq!(unwrap(&update), None);
    }

    #[test]
    fn without_a_login_s_address_the_command_runs_as_it_is() {
        let via = Via::new(8443);
        via.set(true);
        let wrapped = via.wrap(argv(&["echo", "as", "it is"]));
        let syntax = std::process::Command::new("sh")
            .args(["-n", "-c", &wrapped[2]])
            .status()
            .expect("sh runs");
        assert!(syntax.success());
        let output = std::process::Command::new(&wrapped[0])
            .args(&wrapped[1..])
            .env_remove("SSH_CONNECTION")
            .output()
            .expect("sh runs");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"as it is\n");
    }
}
