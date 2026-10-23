//! `Accept-Encoding` negotiation (RFC 9110 §12.5.3) over the variants an asset has.

use axum::http::HeaderValue;
use axum::http::header::GetAll;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Brotli,
    Gzip,
    Identity,
}

/// Picks the encoding with the highest q-value among the available ones, preferring
/// br > gzip > identity on ties. Without the header, or when nothing offered is acceptable,
/// the answer is identity (RFC 9110 lets a server send it rather than a 406).
pub(crate) fn negotiate(
    header: GetAll<'_, HeaderValue>,
    has_brotli: bool,
    has_gzip: bool,
) -> Encoding {
    let mut values = header.into_iter().peekable();
    if values.peek().is_none() {
        return Encoding::Identity;
    }
    // q-values in thousandths; `None` means the coding was not listed.
    let (mut br, mut gzip, mut identity, mut any) = (None, None, None, None);
    for item in values
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
    {
        let mut parts = item.split(';');
        let coding = parts.next().unwrap_or_default().trim();
        let Some(q) = weight(parts) else { continue };
        let slot = if coding.eq_ignore_ascii_case("br") {
            &mut br
        } else if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
            &mut gzip
        } else if coding.eq_ignore_ascii_case("identity") {
            &mut identity
        } else if coding == "*" {
            &mut any
        } else {
            continue;
        };
        *slot = Some(slot.map_or(q, |seen: u16| seen.max(q)));
    }
    let offered =
        |q: Option<u16>, available: bool| if available { q.or(any).unwrap_or(0) } else { 0 };
    let candidates = [
        (Encoding::Brotli, offered(br, has_brotli)),
        (Encoding::Gzip, offered(gzip, has_gzip)),
        // Identity stays acceptable unless excluded explicitly or through `*`, but when the client
        // does not list it, it ranks below every coding the client did ask for.
        (Encoding::Identity, identity.or(any).unwrap_or(1)),
    ];
    // `max_by_key` keeps the last maximum, so iterate in reverse preference order.
    candidates
        .into_iter()
        .rev()
        .filter(|&(_, q)| q > 0)
        .max_by_key(|&(_, q)| q)
        .map_or(Encoding::Identity, |(encoding, _)| encoding)
}

/// The `q` parameter in thousandths (1000 when absent), or `None` if it is malformed.
fn weight<'a>(params: impl Iterator<Item = &'a str>) -> Option<u16> {
    let mut q = 1000;
    for param in params {
        let (name, value) = param.split_once('=')?;
        if name.trim().eq_ignore_ascii_case("q") {
            q = parse_qvalue(value.trim())?;
        }
    }
    Some(q)
}

/// `qvalue = ( "0" [ "." 0*3DIGIT ] ) / ( "1" [ "." 0*3("0") ] )`
fn parse_qvalue(value: &str) -> Option<u16> {
    let (int, frac) = value.split_once('.').unwrap_or((value, ""));
    if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let thousandths = frac
        .bytes()
        .chain(std::iter::repeat(b'0'))
        .take(3)
        .fold(0, |acc, b| acc * 10 + u16::from(b - b'0'));
    match int {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(1000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header::ACCEPT_ENCODING};

    use super::{Encoding, negotiate, parse_qvalue};

    fn pick(values: &[&'static str], has_brotli: bool, has_gzip: bool) -> Encoding {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(ACCEPT_ENCODING, HeaderValue::from_static(value));
        }
        negotiate(headers.get_all(ACCEPT_ENCODING), has_brotli, has_gzip)
    }

    #[test]
    fn prefers_brotli_then_gzip() {
        assert_eq!(
            pick(&["gzip, deflate, br, zstd"], true, true),
            Encoding::Brotli
        );
        assert_eq!(pick(&["gzip, deflate, br"], false, true), Encoding::Gzip);
        assert_eq!(pick(&["gzip, deflate"], true, true), Encoding::Gzip);
        assert_eq!(
            pick(&["br;q=0.9, gzip;q=0.9"], true, true),
            Encoding::Brotli
        );
        assert_eq!(pick(&["*"], true, true), Encoding::Brotli);
    }

    #[test]
    fn honors_q_values() {
        assert_eq!(pick(&["br;q=0.5, gzip"], true, true), Encoding::Gzip);
        assert_eq!(pick(&["br;q=0, gzip;q=0"], true, true), Encoding::Identity);
        assert_eq!(
            pick(&["identity;q=0.5, gzip;q=0.4"], true, true),
            Encoding::Identity
        );
        assert_eq!(pick(&[" BR ; Q=1.000 "], true, true), Encoding::Brotli);
        assert_eq!(pick(&["*;q=0, gzip"], true, true), Encoding::Gzip);
    }

    #[test]
    fn falls_back_to_identity() {
        assert_eq!(pick(&[], true, true), Encoding::Identity);
        assert_eq!(pick(&[""], true, true), Encoding::Identity);
        assert_eq!(pick(&["deflate"], true, true), Encoding::Identity);
        assert_eq!(pick(&["br"], false, true), Encoding::Identity);
        // Nothing acceptable is available: identity rather than a 406.
        assert_eq!(
            pick(&["identity;q=0, br"], false, false),
            Encoding::Identity
        );
        // A malformed q-value drops that entry.
        assert_eq!(pick(&["gzip;q=2"], true, true), Encoding::Identity);
    }

    #[test]
    fn understands_aliases_and_repeated_headers() {
        assert_eq!(pick(&["x-gzip"], true, true), Encoding::Gzip);
        assert_eq!(pick(&["deflate", "GZIP"], false, true), Encoding::Gzip);
    }

    #[test]
    fn parses_qvalues_strictly() {
        assert_eq!(parse_qvalue("1"), Some(1000));
        assert_eq!(parse_qvalue("1.000"), Some(1000));
        assert_eq!(parse_qvalue("0.5"), Some(500));
        assert_eq!(parse_qvalue("0.125"), Some(125));
        assert_eq!(parse_qvalue("0"), Some(0));
        for bad in ["1.5", "2", "0.1234", "-0", ".5", "0.x", ""] {
            assert_eq!(parse_qvalue(bad), None, "{bad:?}");
        }
    }
}
