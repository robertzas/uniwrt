use uniwrt_model::blocklists::parse_header;

use super::compile;

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|&n| n.to_owned()).collect()
}

fn lines(text: &[u8]) -> Vec<String> {
    String::from_utf8(text.to_vec())
        .unwrap()
        .lines()
        .skip(1)
        .map(str::to_owned)
        .collect()
}

#[test]
fn lists_merge_into_sorted_topmost_domains_with_a_header() {
    let hosts = b"0.0.0.0 ads.example.com\n0.0.0.0 x.ads.example.com\n0.0.0.0 B.example.net\n";
    let plain = b"ads.example.com\nexample.net\ndeep.a.b.example.org\n";
    let out = compile(&[hosts, plain], &[], &[], u32::MAX);
    assert_eq!(
        lines(&out.text),
        [
            "server=/ads.example.com/",
            "server=/deep.a.b.example.org/",
            "server=/example.net/",
        ]
    );
    assert_eq!((out.domains, out.cut), (3, 0));
    let first = std::str::from_utf8(&out.text)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    assert_eq!(parse_header(first), Some((out.sha256.clone(), 3)));
    // The same domains in another order compile to the same file.
    let again = compile(&[plain, hosts], &[], &[], u32::MAX);
    assert_eq!(again.text, out.text);
}

#[test]
fn allowed_domains_leave_and_come_back_under_a_blocked_parent() {
    let list = b"tracker.example.com\ncdn.tracker.example.com\nexample.org\nkeep.example.net\n";
    let allow = strings(&[
        "tracker.example.com",
        "good.example.org",
        "Other.example.io",
    ]);
    let deny = strings(&["own.example.io", "bad.tracker.example.com"]);
    let out = compile(&[list], &allow, &deny, u32::MAX);
    assert_eq!(
        lines(&out.text),
        [
            // Allowed under the user's own block, which wins.
            "server=/bad.tracker.example.com/",
            "server=/example.org/",
            "server=/good.example.org/#",
            "server=/keep.example.net/",
            "server=/own.example.io/",
        ]
    );
    assert_eq!(out.domains, 4);
}

#[test]
fn the_cap_keeps_own_blocks_then_lists_in_order() {
    let first = b"a.example.com\nb.example.com\n";
    let second = b"c.example.com\nd.example.com\nsub.own.example.com\n";
    let deny = strings(&["own.example.com"]);
    let out = compile(&[first, second], &[], &deny, 3);
    assert_eq!(
        lines(&out.text),
        [
            "server=/a.example.com/",
            "server=/b.example.com/",
            "server=/own.example.com/",
        ]
    );
    assert_eq!((out.domains, out.cut), (3, 2));
    // A child from an earlier list lends its rank to the later parent that drops it.
    let parent_late = b"x.example.com\nexample.net\n";
    let child_early = b"y.example.net\n";
    let out = compile(&[child_early, parent_late], &[], &[], 1);
    assert_eq!(lines(&out.text), ["server=/example.net/"]);
}

#[test]
fn nothing_compiles_to_a_header_alone() {
    let out = compile(&[], &[], &[], 0);
    assert_eq!(out.domains, 0);
    assert_eq!(std::str::from_utf8(&out.text).unwrap().lines().count(), 1);
}
