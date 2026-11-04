//! Properties: what validates round-trips through text; any text either fails or parses into a
//! package that round-trips; a diff is empty exactly when two packages mean the same, and
//! applying it to the old package gives the new one.

use std::collections::BTreeMap;

use proptest::{collection::vec, option, prelude::*, sample::select};

use crate::{Package, Section, Value, apply, diff, parse, parse_export};

/// Values that exercise quoting: quotes, `#`, `;`, `\`, whitespace, newlines, non-ASCII.
fn text() -> impl Strategy<Value = String> {
    "[ab'\"#;\\\\ \t\r\n\u{e9}\u{1f6dc}]{0,5}"
}

fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        "[a-c'#;\\\\ \n]{1,4}".prop_map(Value::Single),
        text().prop_filter_map("an option holds something", |v| {
            (!v.is_empty()).then_some(Value::Single(v))
        }),
        vec(text(), 1..4).prop_map(Value::List),
    ]
}

fn section() -> impl Strategy<Value = Section> {
    let kinds = select(vec!["rule", "zone", "wifi-iface", "a.b", "t'#;\\\"x"]);
    let names = option::weighted(0.4, select(vec!["lan", "wan", "n_2", "cfg0123"]));
    let options = vec(
        (
            select(vec!["name", "src", "dest", "proto", "Up_2"]),
            value(),
        ),
        0..5,
    );
    (kinds, names, options).prop_map(|(kind, name, options)| {
        let mut section = Section {
            kind: kind.into(),
            name: name.map(Into::into),
            options: vec![],
        };
        for (name, value) in options {
            section.set(name, value);
        }
        section
    })
}

/// A valid package: named sections are unique.
fn package() -> impl Strategy<Value = Package> {
    vec(section(), 0..8).prop_map(|sections| {
        let mut package = Package::new("p");
        for section in sections {
            if section
                .name
                .as_deref()
                .is_none_or(|name| package.section(name).is_none())
            {
                package.sections.push(section);
            }
        }
        package
    })
}

#[derive(Debug, Clone)]
enum Edit {
    Remove(usize),
    Insert(usize, Section),
    Set(usize, &'static str, Value),
    Unset(usize, &'static str),
    Swap(usize, usize),
    Retype(usize, &'static str),
}

fn edit() -> impl Strategy<Value = Edit> {
    let option = select(vec!["name", "src", "dest", "proto"]);
    prop_oneof![
        any::<usize>().prop_map(Edit::Remove),
        (any::<usize>(), section()).prop_map(|(i, s)| Edit::Insert(i, s)),
        (any::<usize>(), option.clone(), value()).prop_map(|(i, o, v)| Edit::Set(i, o, v)),
        (any::<usize>(), option).prop_map(|(i, o)| Edit::Unset(i, o)),
        (any::<usize>(), any::<usize>()).prop_map(|(i, j)| Edit::Swap(i, j)),
        (any::<usize>(), select(vec!["rule", "zone"])).prop_map(|(i, k)| Edit::Retype(i, k)),
    ]
}

/// `a` and an edited copy of it.
fn related() -> impl Strategy<Value = (Package, Package)> {
    (package(), vec(edit(), 0..6)).prop_map(|(old, edits)| {
        let mut new = old.clone();
        for edit in edits {
            let len = new.sections.len();
            match edit {
                Edit::Insert(i, s) => {
                    if s.name
                        .as_deref()
                        .is_none_or(|name| new.section(name).is_none())
                    {
                        new.sections.insert(i % (len + 1), s);
                    }
                }
                _ if len == 0 => {}
                Edit::Remove(i) => {
                    new.sections.remove(i % len);
                }
                Edit::Set(i, o, v) => new.sections[i % len].set(o, v),
                Edit::Unset(i, o) => {
                    new.sections[i % len].remove(o);
                }
                Edit::Swap(i, j) => new.sections.swap(i % len, j % len),
                Edit::Retype(i, k) => k.clone_into(&mut new.sections[i % len].kind),
            }
        }
        (old, new)
    })
}

/// What a package means: per type, its sections in order, each with its options as a map.
type Meaning = BTreeMap<String, Vec<(Option<String>, BTreeMap<String, Value>)>>;

fn meaning(package: &Package) -> Meaning {
    let mut out = Meaning::new();
    for s in &package.sections {
        let options = s
            .options
            .iter()
            .map(|e| (e.name.clone(), e.value.clone()))
            .collect();
        out.entry(s.kind.clone())
            .or_default()
            .push((s.name.clone(), options));
    }
    out
}

/// The package with its options reversed and its types' sections interleaved differently.
fn shuffled(package: &Package) -> Package {
    let mut sections = package.sections.clone();
    for s in &mut sections {
        s.options.reverse();
    }
    let mut kinds: Vec<&str> = sections.iter().map(|s| s.kind.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds.reverse();
    let mut out = Package::new(package.name.clone());
    for kind in kinds {
        out.sections
            .extend(sections.iter().filter(|s| s.kind == kind).cloned());
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn round_trips(p in package()) {
        prop_assert_eq!(p.validate(), Ok(()));
        prop_assert_eq!(parse("p", &p.to_file()), Ok(p.clone()));
        prop_assert_eq!(parse_export(&p.to_export()), Ok(vec![p]));
    }

    #[test]
    fn any_text_parses_or_fails(t in "(config|option|list|package|c|o|l|[ #;'\"\\\\\n\t\r]|a|b|-){0,24}") {
        if let Ok(p) = parse("p", &t) {
            prop_assert_eq!(p.validate(), Ok(()));
            prop_assert_eq!(parse("p", &p.to_file()), Ok(p));
        }
    }

    #[test]
    fn diff_means_the_change((a, b) in related()) {
        let d = diff(&a, &b);
        prop_assert_eq!(d.is_empty(), meaning(&a) == meaning(&b), "{}", d);
        let applied = apply(&a, &d);
        prop_assert!(applied.is_ok(), "{:?}\n{}", applied, d);
        prop_assert_eq!(meaning(&applied.unwrap()), meaning(&b), "{}", d);
    }

    #[test]
    fn diff_of_strangers(a in package(), b in package()) {
        let d = diff(&a, &b);
        prop_assert_eq!(d.is_empty(), meaning(&a) == meaning(&b));
        prop_assert_eq!(meaning(&apply(&a, &d).unwrap()), meaning(&b), "{}", d);
    }

    #[test]
    fn same_meaning_no_diff(a in package()) {
        prop_assert!(diff(&a, &a).is_empty());
        let d = diff(&a, &shuffled(&a));
        prop_assert!(d.is_empty(), "{}", d);
    }
}
