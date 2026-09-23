// Decimal strings preserve source precision; ambiguous punctuation is refused.
pub(crate) fn valid_currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

pub(crate) fn valid_amount(value: &str) -> bool {
    if value.is_empty() || value.len() > 24 {
        return false;
    }
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.len() <= 18
        && (whole == "0" || !whole.starts_with('0'))
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|part| {
            !part.is_empty() && part.len() <= 6 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn canonical(value: &str) -> String {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    }
}

fn numeric_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '.' | ',' | ' ' | '\u{a0}' | '\u{202f}')
}

fn spacing_or_symbol(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '$' | '€' | '£' | '¥' | '₹' | '₩')
}

fn normalize_with_decimal(value: &str, decimal: char) -> Option<String> {
    let value = value.replace(['\u{a0}', '\u{202f}'], " ");
    let mut parts = value.split(decimal);
    let whole = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some()
        || fraction.is_some_and(|part| {
            part.is_empty() || part.len() > 6 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    let digits = if let Some(separator) = whole.chars().find(|ch| !ch.is_ascii_digit()) {
        if !matches!(separator, ',' | '.' | ' ') {
            return None;
        }
        let groups: Vec<_> = whole.split(separator).collect();
        if groups[0].is_empty()
            || groups[0].len() > 3
            || groups.iter().skip(1).any(|group| group.len() != 3)
            || groups
                .iter()
                .any(|group| !group.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return None;
        }
        groups.concat()
    } else {
        whole.to_owned()
    };
    let normalized = canonical(&match fraction {
        Some(fraction) => format!("{digits}.{fraction}"),
        None => digits,
    });
    valid_amount(&normalized).then_some(normalized)
}

fn unambiguous_amount(token: &str, amount: &str) -> bool {
    let token = token.trim();
    if token.is_empty()
        || token.len() > 40
        || !token.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || !token.chars().last().is_some_and(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    let dot = normalize_with_decimal(token, '.');
    let comma = normalize_with_decimal(token, ',');
    let normalized = match (dot, comma) {
        (Some(first), Some(second)) if first == second => first,
        (Some(value), None) | (None, Some(value)) => value,
        _ => return false,
    };
    normalized == canonical(amount)
}

pub(crate) fn supports_money(text: &str, amount: &str, currency: &str) -> bool {
    if !valid_amount(amount) || !valid_currency(currency) {
        return false;
    }
    text.match_indices(currency).any(|(index, _)| {
        let before = &text[..index];
        let after = &text[index + currency.len()..];
        if before
            .chars()
            .last()
            .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
            || after
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
        {
            return false;
        }
        let left = before.trim_end_matches(spacing_or_symbol);
        let start = left
            .char_indices()
            .rfind(|(_, ch)| !numeric_char(*ch))
            .map_or(0, |(index, ch)| index + ch.len_utf8());
        let start = start + left[start..].len() - left[start..].trim_start().len();
        let prefix = &left[..start];
        let signed = prefix
            .trim_end_matches(spacing_or_symbol)
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '-' | '−' | '+' | '('));
        if !signed
            && !prefix
                .chars()
                .last()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && unambiguous_amount(&left[start..], amount)
        {
            return true;
        }
        if before
            .trim_end_matches(spacing_or_symbol)
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '-' | '−' | '+' | '('))
        {
            return false;
        }
        let right = after.trim_start_matches(spacing_or_symbol);
        let end = right.find(|ch| !numeric_char(ch)).unwrap_or(right.len());
        let right_boundary = right[end..].chars().next();
        !right_boundary.is_some_and(|ch| {
            ch.is_alphanumeric() || matches!(ch, '_' | '%' | '‰' | '-' | '−' | '–' | '—' | '+')
        }) && unambiguous_amount(&right[..end], amount)
    })
}

/// Currency codes a displayed amount may carry beside its number.
const CURRENCY_CODES: [&str; 12] = [
    "USD", "EUR", "GBP", "PLN", "CHF", "CAD", "AUD", "JPY", "SEK", "NOK", "DKK", "CZK",
];
const CURRENCY_SYMBOLS: [&str; 4] = ["$", "€", "£", "zł"];

fn spacing(ch: char) -> bool {
    matches!(ch, ' ' | '\u{a0}' | '\u{202f}')
}

/// The marker ending `before` (a symbol, or a closed code at a word start),
/// with at most one space between it and the number: where it starts.
fn marker_before(before: &str) -> Option<usize> {
    let trimmed = before
        .strip_suffix(|ch: char| spacing(ch))
        .unwrap_or(before);
    CURRENCY_SYMBOLS
        .iter()
        .chain(&CURRENCY_CODES)
        .find(|marker| {
            trimmed.ends_with(**marker)
                && (marker.chars().all(|ch| !ch.is_ascii_alphabetic())
                    || !trimmed[..trimmed.len() - marker.len()]
                        .chars()
                        .last()
                        .is_some_and(char::is_alphanumeric))
        })
        .map(|marker| trimmed.len() - marker.len())
}

/// The marker starting `after`, with at most one space before it: where it ends.
fn marker_after(after: &str) -> Option<usize> {
    let skipped = after.len()
        - after
            .strip_prefix(|ch: char| spacing(ch))
            .unwrap_or(after)
            .len();
    let rest = &after[skipped..];
    CURRENCY_SYMBOLS
        .iter()
        .chain(&CURRENCY_CODES)
        .find(|marker| {
            rest.starts_with(**marker)
                && !rest[marker.len()..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric)
        })
        .map(|marker| skipped + marker.len())
}

/// The one currency amount `text` displays, marker included, such as
/// "$349.99" in "Tower Bridge $349.99 New"; none when it shows no amount or
/// more than one.
pub(crate) fn single_currency_amount(text: &str) -> Option<&str> {
    let mut found = None;
    let mut index = 0;
    while index < text.len() {
        let Some(offset) = text[index..].find(|ch: char| ch.is_ascii_digit()) else {
            break;
        };
        let start = index + offset;
        let mut end = start;
        let mut chars = text[start..].char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            let continues = ch.is_ascii_digit()
                || ((matches!(ch, '.' | ',') || spacing(ch))
                    && chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()));
            if !continues {
                break;
            }
            end = start + at + ch.len_utf8();
        }
        index = end.max(start + 1);
        let span = match (marker_before(&text[..start]), marker_after(&text[end..])) {
            (Some(from), _) => from..end,
            (None, Some(to)) => start..end + to,
            (None, None) => continue,
        };
        if found.replace(span).is_some() {
            return None;
        }
    }
    found.map(|span| &text[span])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_displayed_text_yields_its_one_currency_amount_or_none() {
        for (text, amount) in [
            ("Tower Bridge $349.99 New", Some("$349.99")),
            ("Paris – City of Love $79.99", Some("$79.99")),
            ("$39.99", Some("$39.99")),
            ("Cena 1\u{a0}299,00 zł brutto", Some("1\u{a0}299,00 zł")),
            ("£51.77", Some("£51.77")),
            ("Price: USD 20", Some("USD 20")),
            ("12,50 € pro Stück", Some("12,50 €")),
            ("Tower Bridge 21067", None),
            ("Was $59.99, now $39.99", None),
            ("SKU 20 USDA", None),
            ("Pieces:3745", None),
        ] {
            assert_eq!(single_currency_amount(text), amount, "{text}");
        }
    }

    #[test]
    fn money_requires_adjacent_explicit_currency_and_unambiguous_decimal_evidence() {
        for (text, amount, currency) in [
            ("Price $20.00 USD", "20.00", "USD"),
            ("USD 1,299.00", "1299", "USD"),
            ("1.299,50 EUR", "1299.50", "EUR"),
            ("1\u{202f}299,50 EUR", "1299.50", "EUR"),
            ("EUR 0,99", "0.99", "EUR"),
            ("20USD", "20", "USD"),
        ] {
            assert!(supports_money(text, amount, currency), "{text}");
        }
        for (text, amount, currency) in [
            ("$20.00", "20", "USD"),
            ("USD 20", "21", "USD"),
            ("20 EUR", "20", "USD"),
            ("-20 USD", "20", "USD"),
            ("−20 USD", "20", "USD"),
            ("USD -20", "20", "USD"),
            ("-$20 USD", "20", "USD"),
            ("-USD 20", "20", "USD"),
            ("(20 USD)", "20", "USD"),
            ("1,299 USD", "1299", "USD"),
            ("1,299 USD", "1.299", "USD"),
            ("USD 1,29,900", "129900", "USD"),
            ("USD 20_000", "20", "USD"),
            ("USD 20%", "20", "USD"),
            ("USD 20–30", "20", "USD"),
            ("USD 20-30", "20", "USD"),
            ("ABC20USD", "20", "USD"),
            ("20 USDA", "20", "USD"),
            ("Price 20.00. Currency USD", "20", "USD"),
        ] {
            assert!(!supports_money(text, amount, currency), "{text}");
        }
        for amount in ["NaN", "1e3", "-1", "+1", "01", "1.", ".5", "0.0000001"] {
            assert!(!valid_amount(amount), "{amount}");
        }
    }
}
