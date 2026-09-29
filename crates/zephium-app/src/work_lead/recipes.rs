//! Site recipes: a site's own results page for a search, built from the
//! lead's typed search fields, so a page agent starts on the results instead
//! of a date picker. Templates are data, each checked by hand against the
//! live site (2026-09-29); form filling stays the fallback.
use serde_json::Value;

/// What the lead searches for, as typed fields. Dates are `YYYY-MM-DD`,
/// airports IATA codes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SiteSearch {
    pub place: Option<String>,
    pub checkin: Option<String>,
    pub checkout: Option<String>,
    pub adults: Option<u8>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub depart: Option<String>,
    pub back: Option<String>,
    pub cabin: Option<Cabin>,
    pub query: Option<String>,
    pub currency: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cabin {
    Economy,
    Premium,
    Business,
    First,
}

/// What a recipe searches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Stays,
    Flights,
    Products,
}

/// One site's results page. `{field}` is required; `[...]` is kept only
/// when every field inside it is present. Field names may carry an encoding:
/// `{place:slug}` (Airbnb's path form), `{place:q}` (a query value).
struct Recipe {
    site: &'static str,
    name: &'static str,
    kind: Kind,
    /// Stays at least this long use this recipe; shorter ones the next.
    min_nights: u16,
    max_nights: u16,
    /// A site that answers a fresh browser with a consent page it cannot
    /// leave (Google) never leads, even when the part names it.
    leads: bool,
    template: &'static str,
}

const RECIPES: &[Recipe] = &[
    Recipe {
        site: "airbnb.com",
        name: "Airbnb",
        kind: Kind::Stays,
        min_nights: 28,
        max_nights: u16::MAX,
        leads: true,
        template: "https://www.airbnb.com/s/{place:slug}/homes?date_picker_type=monthly_stay&monthly_start_date={checkin}&monthly_length={months}&monthly_end_date={checkout}&adults={adults}[&currency={currency}]",
    },
    Recipe {
        site: "airbnb.com",
        name: "Airbnb",
        kind: Kind::Stays,
        min_nights: 1,
        max_nights: 27,
        leads: true,
        template: "https://www.airbnb.com/s/{place:slug}/homes?checkin={checkin}&checkout={checkout}&adults={adults}[&currency={currency}]",
    },
    Recipe {
        site: "booking.com",
        name: "Booking.com",
        kind: Kind::Stays,
        min_nights: 1,
        max_nights: u16::MAX,
        leads: true,
        template: "https://www.booking.com/searchresults.html?ss={place:q}&checkin={checkin}&checkout={checkout}&group_adults={adults}&no_rooms=1&lang=en-us[&selected_currency={currency}]",
    },
    Recipe {
        site: "kayak.com",
        name: "Kayak",
        kind: Kind::Flights,
        min_nights: 0,
        max_nights: u16::MAX,
        leads: true,
        template: "https://www.kayak.com/flights/{from}-{to}/{depart}[/{back}]{cabin_path}{party_path}?sort=bestflight_a",
    },
    Recipe {
        site: "google.com",
        name: "Google Flights",
        kind: Kind::Flights,
        min_nights: 0,
        max_nights: u16::MAX,
        leads: false,
        template: "https://www.google.com/travel/flights?q=Flights%20from%20{from:q}%20to%20{to:q}%20on%20{depart}{trip:q}{party:q}{cabin_class:q}&hl=en[&curr={currency}]",
    },
    Recipe {
        site: "amazon.com",
        name: "Amazon",
        kind: Kind::Products,
        min_nights: 0,
        max_nights: u16::MAX,
        leads: true,
        template: "https://www.amazon.com/s?k={query:q}",
    },
    Recipe {
        site: "lego.com",
        name: "LEGO",
        kind: Kind::Products,
        min_nights: 0,
        max_nights: u16::MAX,
        leads: true,
        template: "https://www.lego.com/en-us/search?q={query:q}",
    },
];

fn text(args: &Value, key: &str, max: usize) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else if value.chars().count() > max || value.contains(['\n', '<', '>', '"']) {
                Err(format!(
                    "search.{key} is one line of at most {max} characters"
                ))
            } else {
                Ok(Some(value.to_owned()))
            }
        }
        Some(_) => Err(format!("search.{key} is a string")),
    }
}

fn date(args: &Value, key: &str) -> Result<Option<String>, String> {
    let Some(value) = text(args, key, 10)? else {
        return Ok(None);
    };
    days(&value)
        .map(|_| Some(value))
        .ok_or_else(|| format!("search.{key} is a date as YYYY-MM-DD"))
}

fn airport(args: &Value, key: &str) -> Result<Option<String>, String> {
    let Some(value) = text(args, key, 3)? else {
        return Ok(None);
    };
    let code = value.to_ascii_uppercase();
    if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()) {
        Ok(Some(code))
    } else {
        Err(format!(
            "search.{key} is a three-letter IATA airport code such as WAW"
        ))
    }
}

/// A `search` argument as typed fields, or the rule it broke.
pub(crate) fn parse(args: Option<&Value>) -> Result<Option<SiteSearch>, String> {
    let Some(args) = args.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    if !args.is_object() {
        return Err("search is an object of typed fields".into());
    }
    let adults = match args.get("adults") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .filter(|count| (1..=16).contains(count))
                .map(|count| count as u8)
                .ok_or("search.adults is a whole number from 1 to 16")?,
        ),
    };
    let cabin = match text(args, "cabin", 16)?.as_deref() {
        None => None,
        Some("economy") => Some(Cabin::Economy),
        Some("premium") => Some(Cabin::Premium),
        Some("business") => Some(Cabin::Business),
        Some("first") => Some(Cabin::First),
        Some(_) => return Err("search.cabin is economy, premium, business or first".into()),
    };
    let currency = text(args, "currency", 3)?
        .map(|code| code.to_ascii_uppercase())
        .filter(|code| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()));
    let search = SiteSearch {
        place: text(args, "place", 80)?,
        checkin: date(args, "checkin")?,
        checkout: date(args, "checkout")?,
        adults,
        from: airport(args, "from")?,
        to: airport(args, "to")?,
        depart: date(args, "depart")?,
        back: date(args, "return")?,
        cabin,
        query: text(args, "query", 120)?,
        currency,
    };
    if let (Some(checkin), Some(checkout)) = (&search.checkin, &search.checkout) {
        if days(checkout) <= days(checkin) {
            return Err("search.checkout comes after search.checkin".into());
        }
    }
    if let (Some(depart), Some(back)) = (&search.depart, &search.back) {
        if days(back) < days(depart) {
            return Err("search.return comes on or after search.depart".into());
        }
    }
    Ok((search != SiteSearch::default()).then_some(search))
}

/// Days since 1970 for a valid `YYYY-MM-DD`.
fn days(date: &str) -> Option<i64> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| date.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(2000..=2100).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > lengths[(month - 1) as usize]
    {
        return None;
    }
    // Days from civil, after Howard Hinnant.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn query(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Airbnb's place path: "San Francisco, CA" becomes "San-Francisco--CA".
fn slug(value: &str) -> String {
    let path = value
        .split(',')
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join("-"))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("--");
    query(&path)
}

impl SiteSearch {
    fn nights(&self) -> Option<i64> {
        Some(days(self.checkout.as_deref()?)? - days(self.checkin.as_deref()?)?)
    }

    /// One field's value in the template's encoding; None when absent.
    fn field(&self, name: &str) -> Option<String> {
        let (name, encoding) = name.split_once(':').unwrap_or((name, ""));
        let value = match name {
            "place" => self.place.clone()?,
            "checkin" => self.checkin.clone()?,
            "checkout" => self.checkout.clone()?,
            "adults" => self.adults.unwrap_or(1).to_string(),
            "months" => (self.nights()? as f64 / 30.0)
                .round()
                .clamp(1.0, 12.0)
                .to_string(),
            "from" => self.from.clone()?,
            "to" => self.to.clone()?,
            "depart" => self.depart.clone()?,
            "back" => self.back.clone()?,
            "query" => self.query.clone()?,
            "currency" => self.currency.clone()?,
            "trip" => match &self.back {
                Some(back) => format!(" through {back}"),
                None => " oneway".into(),
            },
            "party" => match self.adults.unwrap_or(1) {
                1 => String::new(),
                count => format!(" for {count} adults"),
            },
            "cabin_class" => match self.cabin {
                None | Some(Cabin::Economy) => String::new(),
                Some(Cabin::Premium) => " premium economy class".into(),
                Some(Cabin::Business) => " business class".into(),
                Some(Cabin::First) => " first class".into(),
            },
            "cabin_path" => match self.cabin {
                None | Some(Cabin::Economy) => String::new(),
                Some(Cabin::Premium) => "/premium".into(),
                Some(Cabin::Business) => "/business".into(),
                Some(Cabin::First) => "/first".into(),
            },
            "party_path" => match self.adults.unwrap_or(1) {
                1 => String::new(),
                count => format!("/{count}adults"),
            },
            _ => return None,
        };
        Some(match encoding {
            "q" => query(&value),
            "slug" => slug(&value),
            _ => value,
        })
    }

    fn kind(&self) -> Option<Kind> {
        if self.from.is_some() && self.to.is_some() && self.depart.is_some() {
            Some(Kind::Flights)
        } else if self.place.is_some() && self.checkin.is_some() && self.checkout.is_some() {
            Some(Kind::Stays)
        } else if self.query.is_some() {
            Some(Kind::Products)
        } else {
            None
        }
    }
}

/// Fills a template, or None when a required field is missing.
fn fill(template: &str, search: &SiteSearch) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find(['{', '[']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('[') {
            let end = rest.find(']')?;
            if let Some(section) = fill(&rest[1..end], search) {
                out.push_str(&section);
            }
            rest = &rest[end + 1..];
        } else {
            let end = rest.find('}')?;
            out.push_str(&search.field(&rest[1..end])?);
            rest = &rest[end + 1..];
        }
    }
    out.push_str(rest);
    Some(out)
}

/// The results pages this search opens: the part's own site first when the
/// table knows it and it can lead, then the same search on the other known
/// sites of its kind, for when that site will not load.
pub(crate) fn pages(search: &SiteSearch, site: Option<&str>) -> Vec<(&'static str, String)> {
    let Some(kind) = search.kind() else {
        return Vec::new();
    };
    let nights = search.nights().unwrap_or(0);
    let fits = |recipe: &&Recipe| {
        recipe.kind == kind
            && (kind != Kind::Stays
                || (i64::from(recipe.min_nights) <= nights
                    && nights <= i64::from(recipe.max_nights)))
    };
    let site = site.map(|site| site.trim_start_matches("www.").to_ascii_lowercase());
    let mut recipes: Vec<&Recipe> = RECIPES.iter().filter(fits).collect();
    recipes.sort_by_key(|recipe| (!recipe.leads, Some(recipe.site) != site.as_deref()));
    recipes
        .into_iter()
        .filter_map(|recipe| Some((recipe.name, fill(recipe.template, search)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn search(value: Value) -> SiteSearch {
        parse(Some(&value)).unwrap().unwrap()
    }

    #[test]
    fn stays_open_on_each_sites_own_results_for_the_dates() {
        let week = search(
            json!({"place": "San Francisco, CA", "checkin": "2027-01-05",
            "checkout": "2027-01-12", "adults": 1}),
        );
        assert_eq!(
            pages(&week, Some("booking.com"))[1],
            (
                "Airbnb",
                "https://www.airbnb.com/s/San-Francisco--CA/homes?checkin=2027-01-05&checkout=2027-01-12&adults=1".to_owned()
            )
        );
        assert_eq!(pages(&week, Some("booking.com"))[0].0, "Booking.com");
        let month = search(
            json!({"place": "San Francisco, CA", "checkin": "2027-01-01",
            "checkout": "2027-02-01", "adults": 1, "currency": "usd"}),
        );
        assert_eq!(
            pages(&month, Some("www.airbnb.com"))[0].1,
            "https://www.airbnb.com/s/San-Francisco--CA/homes?date_picker_type=monthly_stay&monthly_start_date=2027-01-01&monthly_length=1&monthly_end_date=2027-02-01&adults=1&currency=USD"
        );
        // No site named: every known stays site.
        let all = pages(&week, None);
        assert_eq!(
            all.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            ["Airbnb", "Booking.com"]
        );
        assert_eq!(
            all[1].1,
            "https://www.booking.com/searchresults.html?ss=San%20Francisco%2C%20CA&checkin=2027-01-05&checkout=2027-01-12&group_adults=1&no_rooms=1&lang=en-us"
        );
    }

    #[test]
    fn flights_carry_the_trip_party_and_cabin_in_the_address() {
        let round = search(json!({"from": "waw", "to": "SFO", "depart": "2027-01-05",
            "return": "2027-03-20", "currency": "USD"}));
        let pages = pages(&round, None);
        assert_eq!(
            pages[1].1,
            "https://www.google.com/travel/flights?q=Flights%20from%20WAW%20to%20SFO%20on%202027-01-05%20through%202027-03-20&hl=en&curr=USD"
        );
        assert_eq!(
            pages[0].1,
            "https://www.kayak.com/flights/WAW-SFO/2027-01-05/2027-03-20?sort=bestflight_a"
        );
        let business = search(json!({"from": "WAW", "to": "SFO", "depart": "2027-01-05",
            "adults": 2, "cabin": "business"}));
        let urls: Vec<String> = super::pages(&business, None)
            .into_iter()
            .map(|(_, url)| url)
            .collect();
        assert_eq!(
            urls,
            [
                "https://www.kayak.com/flights/WAW-SFO/2027-01-05/business/2adults?sort=bestflight_a",
                "https://www.google.com/travel/flights?q=Flights%20from%20WAW%20to%20SFO%20on%202027-01-05%20oneway%20for%202%20adults%20business%20class&hl=en",
            ]
        );
        assert_eq!(super::pages(&business, Some("kayak.com"))[0].0, "Kayak");
        assert_eq!(super::pages(&business, Some("google.com"))[0].0, "Kayak");
    }

    #[test]
    fn products_search_the_store_and_bad_fields_say_their_rule() {
        let lego = search(json!({"query": "star wars"}));
        assert_eq!(
            pages(&lego, Some("lego.com"))[0],
            (
                "LEGO",
                "https://www.lego.com/en-us/search?q=star%20wars".to_owned()
            )
        );
        assert!(pages(&lego, Some("unknown.shop")).len() == 2);
        for (bad, rule) in [
            (json!({"checkin": "2027-02-30"}), "search.checkin"),
            (json!({"from": "Warsaw"}), "search.from"),
            (json!({"adults": 0}), "search.adults"),
            (
                json!({"checkin": "2027-01-05", "checkout": "2027-01-05"}),
                "search.checkout",
            ),
            (json!({"cabin": "sleeper"}), "search.cabin"),
            (json!("WAW"), "search is"),
        ] {
            assert!(parse(Some(&bad)).unwrap_err().starts_with(rule), "{bad}");
        }
        assert_eq!(parse(Some(&json!({}))).unwrap(), None);
        assert_eq!(days("2027-01-01").unwrap() - days("2026-12-31").unwrap(), 1);
    }
}
