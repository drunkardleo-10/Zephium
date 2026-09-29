---
name: trip-planning
description: Plan a trip end to end: where to stay, how to get there and back, entry rules, and a day-by-day plan with the total cost.
tools: [ask, start_part, create, finish]
---
# Trip planning

## First
- Know the dates (or length and month), where from, who travels and the budget. Ask only what the request and the thread leave open, in one ask with options (for a budget: "Under $2,000", "$2,000–4,000", "No limit"). Never ask what you can find out.
- A known event fixes the dates and the place (a YC batch runs at the YC office in San Francisco; check its dates with one web_search before asking).

## Parts (start them in one turn, they run in parallel)
- **Stay** — browser part on airbnb.com (booking.com when the person prefers hotels), with `search` {place, checkin, checkout, adults}: the helper starts on the site's results for those dates, so no date picker is needed (a month or more opens Airbnb's monthly stays). Brief: the area near where they need to be and the budget per night or per month. It returns three homes as picks with each home's own photos, price and total, rating, and the reason it fits.
- **Flights** — browser part on kayak.com (Google Flights as the second choice), with `search` {from, to, depart, return, adults, cabin}, airports as IATA codes (WAW, SFO) and dates as YYYY-MM-DD; omit return for one way. It returns three to four flights as picks with the route drawn (from, to, times, stops, carrier with its host such as lot.com), price and duration; mark the one you would book as recommended.
- **Entry** — research part: visa or ESTA, passport validity, what to carry, for the traveller's nationality when known. It returns a short list (requirements), each item with its source.
- Add a part only for a real need the request names (a conference ticket, a car, a restaurant).

## Result
- One **plan** from the flight out to the flight back: travel steps for each flight (pick pointing at its flight pick), the stay (pick pointing at its home pick), the event days, entry steps before departure (kind task, with the deadline in when), and the flight home. Advice goes on the step it belongs to ("BART from SFO, about $10, 30 min"). Include a total (flights + stay + known fixed costs) with its label, such as "Estimated total".
- Then the **reply**: headline with the shape of the trip ("6 nights in SoMa, $3,420 in all"), text naming the recommended flight and home and the one thing to do first; figures for total, flight price and nightly price.
- Never make to-dos the agent could do itself ("Compare flights" is the Flights part's job). No guide document unless the person asked for one.
- A part that found nothing leaves its steps without a pick or price: no estimate from a search snippet stands in for a fare or a home, and the reply says in one sentence what is still open. Its row shows the fix.

## Follow-ups
- "Cheaper flights", "a different area", "book this one": run only the part it concerns, then revise that part's picks and the plan. Booking goes through the page and the app's Confirm; never say it is booked until the part reports it done.
