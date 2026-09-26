import type { Entity, EntityFacet, Fact, Picture } from "./types";

type Subject = { name: string; descriptor?: string | null; homepage?: string | null };

/**
 * What kind of thing a subject is, from its own words and where it lives. A
 * closed table, first match wins; a subject nothing names is a product.
 */
const FACETS: readonly (readonly [EntityFacet, RegExp])[] = [
  ["pull_request", /github\.com\/[^/\s]+\/[^/\s]+\/pull\/\d+|\bpull request\b/iu],
  ["repo", /github\.com\/[^/\s]+\/[^/\s]+|gitlab\.com\/|\brepositor(y|ies)\b/iu],
  ["person", /linkedin\.com\/in\//iu],
  [
    "job",
    /\b(jobs?|roles?|hiring|careers?|salary|engineer|developer|designer|manager|internship|remote)\b|greenhouse\.io|lever\.co|ashbyhq|workable|wellfound|indeed\.|linkedin\.com\/jobs/iu,
  ],
  [
    "flight",
    /\b(flights?|airlines?|fares?|nonstop|layovers?|one[- ]way|round[- ]trip)\b|skyscanner|kayak\.|google\.com\/travel\/flights/iu,
  ],
  ["flight", /\b[A-Z]{3}\s?(?:→|->|–|-)\s?[A-Z]{3}\b/u],
  [
    "stay",
    /\b(airbnb|hotels?|hostels?|apartments?|flats?|stays?|lodging|guesthouse|nights?|nightly)\b|airbnb\.|booking\.com|vrbo\./iu,
  ],
  [
    "place",
    /\b(restaurants?|bistro|trattoria|brasserie|cafe|café|bar|sushi|pizzeria|tavern|diner|kitchen|dinner|lunch|brunch|cuisine|menu|michelin)\b|opentable|resy\.|yelp\.|tripadvisor|maps\.google/iu,
  ],
  ["event", /\b(conference|meetup|summit|festival|concert|event)s?\b|eventbrite|lu\.ma/iu],
  ["company", /\b(inc|ltd|gmbh|llc|startup|company)\b|crunchbase\.com/iu],
];

function facetOf(subject: Subject, facts: readonly Fact[]): EntityFacet {
  const text = [
    subject.name,
    subject.descriptor ?? "",
    subject.homepage ?? "",
    ...facts.map((fact) => `${fact.label} ${fact.value}`),
  ].join(" \n ");
  return FACETS.find(([, pattern]) => pattern.test(text))?.[0] ?? "product";
}

const PRICE =
  /\b(price|cost|fare|rate|per night|nightly|total|salary|pay|compensation|rent|fee)\b/iu;
const MONEY = /^[~≈]?\s*(?:[$€£¥]|[A-Z]{3}\s)\s?\d|\d\s?(?:[$€£¥]|zł|PLN|USD|EUR|GBP)\b/u;
const TIME =
  /\b(duration|time|depart(?:ure|s)?|arriv(?:al|es)?|dates?|when|schedule|hours|check[- ]in|stops?)\b/iu;

/** A subject's facts read as a card reads them: its price, its time, then up to four others. */
export function entityOf(
  key: string,
  subject: Subject,
  facts: readonly Fact[],
  extra: {
    element?: string;
    image?: Picture;
    sources?: string[];
    chosen?: boolean;
  } = {},
): Entity {
  let price: string | undefined;
  let time: string | undefined;
  const rest: Fact[] = [];
  for (const fact of facts) {
    if (!price && (PRICE.test(fact.label) || MONEY.test(fact.value.trim()))) price = fact.value;
    else if (!time && TIME.test(fact.label)) time = fact.value;
    else rest.push(fact);
  }
  const descriptor = subject.descriptor?.trim();
  return {
    key,
    ...(extra.element ? { element: extra.element } : {}),
    facet: facetOf(subject, facts),
    name: subject.name.trim(),
    ...(descriptor ? { descriptor } : {}),
    ...(extra.image ? { image: extra.image } : {}),
    ...(subject.homepage ? { homepage: subject.homepage } : {}),
    ...(price ? { price } : {}),
    ...(time ? { time } : {}),
    facts: rest.slice(0, 4),
    ...(extra.sources?.length ? { sources: extra.sources } : {}),
    ...(extra.chosen ? { chosen: true } : {}),
  };
}
