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

#[cfg(test)]
mod tests {
    use super::*;

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
