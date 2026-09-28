import type {
  CodeView,
  DiagramView,
  DiffView,
  DocumentObjectView,
  DraftView,
  FileView,
  FolderView,
  ListView,
  MediaView,
  NoteView,
  ObjectView,
  PageObjectView,
  PicksView,
  PlanView,
  PlotView,
  ReplyView,
  SheetView,
} from "../lib/board/types";

/**
 * Every object as the new runtime fills it, from the acceptance tasks (spec §9).
 * Pictures are the QA profile's admitted images, copied read-only into
 * node_modules/.work-look/objects; without them a picture simply does not load.
 */
const LOOK = "/node_modules/.work-look/objects";
const picture = (name: string, width: number, height: number) => ({
  src: `${LOOK}/${name}`,
  width,
  height,
});

const replySmall: ReplyView = {
  kind: "reply",
  id: "reply-small",
  headline: "30 EUR is about 128 PLN",
  text: "At today's rate of **4.27 PLN** per euro. Card payments usually add 1–3% on top of the mid-market rate.",
  figures: [],
  points: [],
};

const replyFigures: ReplyView = {
  kind: "reply",
  id: "reply-figures",
  headline: "A multi-tenant SaaS on AWS, with the AI work on a queue",
  text: "Users reach a **Next.js** app through Cloudflare. The API checks the tenant on every request and hands slow model calls to workers through **SQS**, so a 20-second completion never blocks a page. Postgres holds the data, with `pgvector` until search outgrows it.",
  figures: [
    { label: "MVP per month", value: "$550–9,500" },
    { label: "At growth", value: "$9,800–153K", note: "model usage is most of it" },
    { label: "Services to run", value: "7" },
  ],
  points: [
    "Tenant id on every row, enforced by row-level security",
    "Timeouts, bounded retries and a budget per tenant",
    "Model providers behind one adapter, so they can change",
  ],
};

const stays: PicksView = {
  kind: "picks",
  id: "stays",
  title: "Homes near YC for the batch",
  facet: "stay",
  items: [
    {
      name: "Lux 2br/2ba next to YC",
      subtitle: "Entire flat · Dogpatch · 2 blocks from YC",
      picture: picture("3a91a46d12.jpg", 1200, 900),
      logo: "airbnb.com",
      url: "https://www.airbnb.com/rooms/965072398758262802",
      price: { display: "$6,900 / month", amount: 6900, currency: "USD" },
      facts: [
        { label: "Bedrooms", value: "2", kind: "text" },
        { label: "Workspace", value: "Office nook", kind: "yes" },
        { label: "Washer", value: "In unit", kind: "yes" },
        { label: "Min. stay", value: "30 nights", kind: "text" },
      ],
      rating: { value: 4.92, max: 5, count: 61 },
      why: "Two blocks from YC, a real desk, and the host takes batch bookings.",
      tags: ["Walk to YC", "Superhost"],
      recommended: true,
    },
    {
      name: "Sunny studio in Mission Bay",
      subtitle: "Entire studio · Mission Bay · 12 min to YC",
      picture: picture("9fb9978c4c.webp", 720, 540),
      logo: "airbnb.com",
      url: "https://www.airbnb.com/rooms/47980567",
      price: { display: "$4,350 / month", amount: 4350, currency: "USD" },
      facts: [
        { label: "Bedrooms", value: "Studio", kind: "text" },
        { label: "Workspace", value: "Desk", kind: "yes" },
        { label: "Washer", value: "In building", kind: "partial" },
        { label: "Min. stay", value: "30 nights", kind: "text" },
      ],
      rating: { value: 4.81, max: 5, count: 138 },
      why: "The cheapest place with a desk inside a 15-minute walk.",
      tags: ["Monthly discount"],
      recommended: false,
    },
    {
      name: "Room in a founders' house",
      subtitle: "Private room · SoMa · 20 min to YC",
      picture: picture("ffa4fad393.webp", 1200, 600),
      logo: "airbnb.com",
      url: "https://www.airbnb.com/rooms/1181130",
      price: { display: "$2,900 / month", amount: 2900, currency: "USD" },
      facts: [
        { label: "Bedrooms", value: "Shared flat", kind: "text" },
        { label: "Workspace", value: "Shared", kind: "partial" },
        { label: "Washer", value: "In unit", kind: "yes" },
        { label: "Private bath", value: "No", kind: "no" },
      ],
      rating: { value: 4.7, max: 5, count: 24 },
      why: "Half the price, and five other founders under one roof.",
      tags: ["Community"],
      recommended: false,
    },
  ],
};

const flights: PicksView = {
  kind: "picks",
  id: "flights",
  title: "Flights to San Francisco",
  facet: "flight",
  items: [
    {
      name: "Lufthansa via Frankfurt",
      logo: "lufthansa.com",
      url: "https://www.lufthansa.com",
      price: { display: "$1,142", amount: 1142, currency: "USD" },
      facts: [
        { label: "Bag", value: "23 kg included", kind: "yes" },
        { label: "Change", value: "Fee applies", kind: "partial" },
      ],
      why: "The shortest trip that lands before the Monday kickoff.",
      tags: [],
      recommended: true,
      when: "Sun 4 Jan",
      route: {
        from: "WAW",
        to: "SFO",
        depart: "07:05",
        arrive: "13:05",
        duration: "15 h 00 m",
        stops: 1,
        via: ["FRA"],
        carrier: "Lufthansa",
        carrierHost: "lufthansa.com",
      },
    },
    {
      name: "KLM via Amsterdam",
      logo: "klm.com",
      url: "https://www.klm.com",
      price: { display: "$1,068", amount: 1068, currency: "USD" },
      facts: [
        { label: "Bag", value: "23 kg included", kind: "yes" },
        { label: "Change", value: "Free", kind: "yes" },
      ],
      tags: [],
      recommended: false,
      when: "Sun 4 Jan",
      route: {
        from: "WAW",
        to: "SFO",
        depart: "06:00",
        arrive: "12:40",
        duration: "15 h 40 m",
        stops: 1,
        via: ["AMS"],
        carrier: "KLM",
        carrierHost: "klm.com",
      },
    },
    {
      name: "LOT and United via Chicago",
      logo: "lot.com",
      url: "https://www.lot.com",
      price: { display: "$986", amount: 986, currency: "USD" },
      facts: [
        { label: "Bag", value: "Carry-on only", kind: "no" },
        { label: "Change", value: "Fee applies", kind: "partial" },
      ],
      tags: ["Cheapest"],
      recommended: false,
      when: "Sun 4 Jan",
      route: {
        from: "WAW",
        to: "SFO",
        depart: "10:25",
        arrive: "19:50",
        duration: "18 h 25 m",
        stops: 2,
        via: ["ORD", "DEN"],
        carrier: "LOT",
        carrierHost: "lot.com",
      },
    },
  ],
};

const products: PicksView = {
  kind: "picks",
  id: "products",
  title: "LEGO Architecture sets",
  facet: "product",
  items: [
    {
      name: "Neuschwanstein Castle",
      subtitle: "LEGO Architecture · 21063",
      picture: picture("c89b72e433.jpg", 1500, 1132),
      logo: "lego.com",
      url: "https://www.lego.com/en-us/product/neuschwanstein-castle-21063",
      price: { display: "$279.99", amount: 279.99, currency: "USD" },
      facts: [
        { label: "Pieces", value: "3,455", kind: "text" },
        { label: "In stock", value: "Yes", kind: "yes" },
        { label: "Build time", value: "about 12 h", kind: "text" },
      ],
      rating: { value: 4.8, max: 5, count: 412 },
      why: "The most model for the money: twelve times the pieces of London.",
      tags: ["18+"],
      recommended: true,
    },
    {
      name: "Sagrada Família",
      subtitle: "LEGO Architecture · 21065",
      picture: picture("b7b16b22a0.png", 703, 800),
      logo: "lego.com",
      url: "https://www.lego.com/en-us/product/sagrada-familia-21065",
      price: { display: "$799.99", amount: 799.99, currency: "USD" },
      facts: [
        { label: "Pieces", value: "12,060", kind: "text" },
        { label: "In stock", value: "Backorder", kind: "partial" },
        { label: "Build time", value: "about 40 h", kind: "text" },
      ],
      rating: { value: 4.9, max: 5, count: 96 },
      tags: ["18+", "Largest"],
      recommended: false,
    },
    {
      name: "London",
      subtitle: "LEGO Architecture · 21034",
      picture: picture("816667de11.jpg", 800, 800),
      logo: "lego.com",
      url: "https://www.lego.com/en-us/product/london-21034",
      price: { display: "$39.99", amount: 39.99, currency: "USD" },
      facts: [
        { label: "Pieces", value: "468", kind: "text" },
        { label: "In stock", value: "Yes", kind: "yes" },
        { label: "Build time", value: "about 2 h", kind: "text" },
      ],
      rating: { value: 4.6, max: 5, count: 1840 },
      tags: ["12+"],
      recommended: false,
    },
  ],
};

const videos: PicksView = {
  kind: "picks",
  id: "videos",
  title: "Lectures to start with",
  facet: "video",
  items: [
    {
      name: "Compilers: lexical analysis",
      subtitle: "Stanford CS143 · Alex Aiken",
      picture: picture("479469f204.jpg", 480, 360),
      logo: "youtube.com",
      url: "https://www.youtube.com/watch?v=FgzyLoSkL5k",
      duration: "17:42",
      facts: [],
      why: "The clearest first hour on scanners, and it matches week 1 of the plan.",
      tags: ["Week 1"],
      recommended: true,
    },
    {
      name: "Crafting Interpreters, chapter 4",
      subtitle: "Robert Nystrom · book",
      logo: "craftinginterpreters.com",
      url: "https://craftinginterpreters.com/scanning.html",
      duration: "45 min read",
      facts: [{ label: "Free online", value: "Yes", kind: "yes" }],
      tags: ["Week 1"],
      recommended: false,
    },
    {
      name: "MIT 6.035 lecture notes",
      subtitle: "MIT OpenCourseWare",
      logo: "ocw.mit.edu",
      url: "https://ocw.mit.edu/courses/6-035-computer-language-engineering-spring-2010/",
      duration: "10 weeks",
      facts: [{ label: "Assignments", value: "With solutions", kind: "yes" }],
      tags: ["Weeks 2–4"],
      recommended: false,
    },
  ],
};

const trip: PlanView = {
  kind: "plan",
  id: "trip",
  title: "Your YC batch trip",
  checkable: true,
  steps: [
    {
      when: "Sun 4 Jan",
      title: "Fly Warsaw → San Francisco",
      detail: "Lufthansa via Frankfurt, lands 13:05. BART from SFO is about $10 and 30 min.",
      kind: "travel",
      cost: "$1,142",
      place: "WAW → SFO",
      pick: { name: "Lufthansa via Frankfurt", logo: "lufthansa.com" },
      done: true,
    },
    {
      when: "Sun 4 Jan",
      title: "Check in near YC",
      detail: "Code arrives by message the day before; the host is on site until 18:00.",
      kind: "stay",
      cost: "$6,900 / mo",
      place: "Dogpatch",
      pick: { name: "Lux 2br/2ba next to YC", picture: picture("3a91a46d12.jpg", 1200, 900) },
    },
    {
      when: "Mon 5 Jan",
      title: "Batch kickoff",
      detail: "Bring your passport and the signed SAFE; doors at 09:30.",
      kind: "event",
      place: "YC, Mission Bay",
    },
    {
      when: "Tue 6 Jan",
      title: "Open a US bank account",
      detail: "Mercury works remotely with the Delaware C-corp documents.",
      kind: "task",
    },
    {
      when: "Wed 7 Jan",
      title: "Get a US SIM",
      detail: "An eSIM from Mint Mobile is $15 for the first month.",
      kind: "task",
      cost: "$15",
    },
    {
      when: "Thu 12 Mar",
      title: "Demo Day",
      kind: "milestone",
      place: "Pier 48",
    },
    {
      when: "Sat 14 Mar",
      title: "Fly home",
      detail: "Return leg of the same Lufthansa booking.",
      kind: "travel",
      place: "SFO → WAW",
    },
  ],
  total: { label: "Trip total", value: "$22,390" },
};

const today: ListView = {
  kind: "list",
  id: "today",
  title: "Today",
  style: "todo",
  items: [
    {
      title: "Send Maya the pricing deck before her 3 pm call",
      due: "Before 15:00",
      priority: "high",
      from: {
        host: "slack.com",
        app: "Slack",
        who: "Maya Chen",
        when: "09:12",
        quote:
          "Could you send me the latest pricing deck before my 3pm with Acme? Their CFO will be on.",
        url: "https://acme.slack.com/archives/C024/p1727",
      },
    },
    {
      title: "Review the onboarding PR",
      detail: "Two comments are still open on the invite flow.",
      from: {
        host: "github.com",
        app: "GitHub",
        who: "jonas-k",
        when: "Yesterday",
        url: "https://github.com/acme/app/pull/482",
      },
    },
    {
      title: "Confirm the dinner with the Accel partners",
      due: "Today",
      from: {
        host: "mail.google.com",
        app: "Gmail",
        who: "Sarah Lin",
        when: "08:40",
        quote: "Are we still on for 7:30 at Nopa? Happy to move it if the batch runs late.",
        url: "https://mail.google.com/mail/u/0/#inbox/FMfcgz",
      },
    },
    {
      title: "Reply to the Stripe Atlas question about your EIN",
      from: {
        host: "mail.google.com",
        app: "Gmail",
        who: "Stripe Atlas",
        when: "Mon",
        url: "https://mail.google.com/mail/u/0/#inbox/QgrcJHs",
      },
    },
  ],
};

const providers: SheetView = {
  kind: "sheet",
  id: "providers",
  title: "Providers for this architecture",
  columns: [
    { label: "Provider", kind: "entity" },
    { label: "From", kind: "money", currency: "USD", best: "min" },
    { label: "Managed Postgres", kind: "yes_no" },
    { label: "Queues", kind: "yes_no" },
    { label: "Edge CDN", kind: "yes_no" },
    { label: "Ops effort", kind: "rating", best: "min" },
    { label: "Regions", kind: "number", best: "max" },
    { label: "Fits here", kind: "tag" },
  ],
  rows: [
    {
      cells: ["AWS", "180", "yes", "yes", "partial", "4/5", "34", "Backend"],
      entity: { logo: "aws.amazon.com" },
    },
    {
      cells: ["Vercel", "20", "partial", "no", "yes", "1/5", "18", "Web app"],
      entity: { logo: "vercel.com" },
    },
    {
      cells: ["Hetzner", "5", "no", "no", "no", "5/5", "6", "Cheap compute"],
      entity: { logo: "hetzner.com" },
    },
    {
      cells: ["Cloudflare", "5", "partial", "yes", "yes", "2/5", "330", "Edge"],
      entity: { logo: "cloudflare.com" },
    },
  ],
  note: "Starting prices for the smallest production tier, checked 28 Sep.",
};

const costs: PlotView = {
  kind: "plot",
  id: "costs",
  title: "Monthly cost at launch",
  spec: {
    kind: "donut",
    x: { label: "Category" },
    y: { label: "Cost", format: "money", currency: "USD" },
    headline: { label: "A month at launch", value: "$1,840" },
    basis: "Midpoints of the MVP estimates; model usage assumed at 2M tokens a day.",
    series: [
      {
        name: "Cost",
        points: [
          { x: "Model API", y: 820 },
          { x: "Database", y: 380 },
          { x: "Hosting and edge", y: 260 },
          { x: "Workers", y: 210 },
          { x: "Monitoring", y: 110 },
          { x: "Email and auth", y: 60 },
        ],
      },
    ],
  },
};

const growth: PlotView = {
  kind: "plot",
  id: "growth",
  title: "Model spend as usage grows",
  spec: {
    kind: "area",
    x: { label: "Month", kind: "category" },
    y: { label: "Spend", format: "money", currency: "USD" },
    headline: { label: "By month 12", value: "$18.4K" },
    basis: "At $3 per million input tokens and 40% cache hits.",
    series: [
      {
        name: "Model API",
        points: [
          "Jan",
          "Feb",
          "Mar",
          "Apr",
          "May",
          "Jun",
          "Jul",
          "Aug",
          "Sep",
          "Oct",
          "Nov",
          "Dec",
        ].map((x, index) => ({ x, y: Math.round(820 * 1.32 ** index) })),
      },
    ],
  },
};

const architecture: DiagramView = {
  kind: "diagram",
  id: "architecture",
  title: "Reference architecture",
  diagram: {
    kind: "diagram",
    nodes: [
      {
        id: "user",
        name: "Browser client",
        kind: "client",
        note: "Web UI, no provider secrets",
        layer: "client",
      },
      {
        id: "edge",
        name: "Cloudflare CDN",
        kind: "edge",
        vendor: "cloudflare.com",
        note: "TLS, caching, bot and rate controls",
        layer: "delivery",
      },
      {
        id: "web",
        name: "Next.js app",
        kind: "client",
        vendor: "vercel.com",
        note: "Server-rendered UI on Vercel",
        layer: "delivery",
      },
      {
        id: "api",
        name: "API service",
        kind: "gateway",
        note: "Tenant checks and quotas",
        layer: "app",
      },
      {
        id: "auth",
        name: "Amazon Cognito",
        kind: "service",
        vendor: "aws.amazon.com",
        note: "OIDC identity per tenant",
        layer: "app",
      },
      {
        id: "queue",
        name: "Amazon SQS",
        kind: "queue",
        vendor: "aws.amazon.com",
        note: "Jobs with a dead-letter queue",
        layer: "async",
      },
      {
        id: "worker",
        name: "AI workers",
        kind: "worker",
        note: "Retries, provider adapter",
        layer: "async",
      },
      {
        id: "model",
        name: "OpenAI API",
        kind: "model",
        vendor: "openai.com",
        note: "Model per task and budget",
        layer: "async",
      },
      {
        id: "db",
        name: "PostgreSQL on RDS",
        kind: "store",
        vendor: "aws.amazon.com",
        note: "Tenant data, jobs, usage",
        layer: "data",
      },
      {
        id: "obs",
        name: "OpenTelemetry",
        kind: "service",
        note: "Logs, traces, alerts",
        layer: "data",
      },
    ],
    edges: [
      { from: "user", to: "edge", label: "HTTPS" },
      { from: "edge", to: "web", label: "serve app" },
      { from: "web", to: "api", label: "signed requests" },
      { from: "api", to: "auth", label: "tenant claims" },
      { from: "api", to: "db", label: "reads, writes" },
      { from: "api", to: "queue", label: "enqueue job" },
      { from: "queue", to: "worker", label: "consume" },
      { from: "worker", to: "model", label: "inference" },
      { from: "worker", to: "db", label: "store result" },
      { from: "api", to: "obs", label: "telemetry" },
      { from: "worker", to: "obs", label: "telemetry" },
    ],
    layers: [
      { id: "client", name: "Client" },
      { id: "delivery", name: "Delivery" },
      { id: "app", name: "Application" },
      { id: "async", name: "Async AI" },
      { id: "data", name: "Data and operations" },
    ],
  },
};

const excerpt: CodeView = {
  kind: "code",
  id: "excerpt",
  title: "Where the invite expires",
  language: "rust",
  path: "crates/invites/src/token.rs",
  start: 41,
  text: `pub fn verify(token: &str, now: OffsetDateTime) -> Result<Invite, InviteError> {
    let claims = decode(token)?;
    if claims.expires_at < now {
        return Err(InviteError::Expired);
    }
    Ok(Invite::from(claims))
}`,
  notes: [{ from: 3, to: 5, text: "Compares against the server clock, not the invite's timezone" }],
};

const fix: DiffView = {
  kind: "diff",
  id: "fix",
  title: "Fix invite expiry across timezones",
  path: "crates/invites/src/token.rs",
  language: "rust",
  summary: "Compare expiry in UTC so invites no longer lapse early east of London.",
  hunks: [
    {
      oldStart: 41,
      newStart: 41,
      lines: [
        {
          op: "ctx",
          text: "pub fn verify(token: &str, now: OffsetDateTime) -> Result<Invite, InviteError> {",
        },
        { op: "ctx", text: "    let claims = decode(token)?;" },
        { op: "del", text: "    if claims.expires_at < now {" },
        {
          op: "add",
          text: "    if claims.expires_at.to_offset(UtcOffset::UTC) < now.to_offset(UtcOffset::UTC) {",
        },
        { op: "ctx", text: "        return Err(InviteError::Expired);" },
        { op: "ctx", text: "    }" },
      ],
    },
    {
      oldStart: 88,
      newStart: 88,
      lines: [
        { op: "ctx", text: "#[test]" },
        { op: "del", text: "fn expires_after_a_week() {" },
        { op: "add", text: "fn expires_after_a_week_in_any_timezone() {" },
        { op: "add", text: "    let warsaw = UtcOffset::from_hms(2, 0, 0).unwrap();" },
        { op: "ctx", text: "    let issued = datetime!(2026-01-04 23:30 UTC);" },
      ],
    },
  ],
};

const guide: DocumentObjectView = {
  kind: "document",
  id: "guide",
  title: "Arriving in San Francisco",
  content: {
    kind: "document",
    paragraphs: [
      "Getting in from SFO",
      "BART runs from the international terminal to Civic Center in about 30 minutes for roughly $10. A ride-share to Dogpatch costs $45–70 and takes as long in traffic.",
      "Your first week",
      "Open a US bank account with Mercury on Tuesday, pick up an eSIM before you land, and keep your passport with you on Monday: YC checks ID at the kickoff.",
    ],
    formatted: {
      version: 1,
      document: {
        type: "doc",
        content: [
          {
            type: "heading",
            attrs: { level: 2 },
            content: [{ type: "text", text: "Getting in from SFO" }],
          },
          {
            type: "paragraph",
            content: [
              {
                type: "text",
                text: "BART runs from the international terminal to Civic Center in about 30 minutes for roughly ",
              },
              { type: "text", text: "$10", marks: [{ type: "bold" }] },
              {
                type: "text",
                text: ". A ride-share to Dogpatch costs $45–70 and takes as long in traffic.",
              },
            ],
          },
          {
            type: "heading",
            attrs: { level: 2 },
            content: [{ type: "text", text: "Your first week" }],
          },
          {
            type: "bulletList",
            content: [
              {
                type: "listItem",
                content: [
                  {
                    type: "paragraph",
                    content: [
                      { type: "text", text: "Open a US bank account with Mercury on Tuesday." },
                    ],
                  },
                ],
              },
              {
                type: "listItem",
                content: [
                  {
                    type: "paragraph",
                    content: [{ type: "text", text: "Pick up an eSIM before you land." }],
                  },
                ],
              },
              {
                type: "listItem",
                content: [
                  {
                    type: "paragraph",
                    content: [
                      {
                        type: "text",
                        text: "Keep your passport with you on Monday: YC checks ID at the kickoff.",
                      },
                    ],
                  },
                ],
              },
            ],
          },
        ],
      },
    },
  },
};

const drafts: DraftView[] = [
  {
    kind: "draft",
    id: "draft-slack",
    destination: "slack",
    to: "#pricing",
    body: "Hey Maya, here's the latest pricing deck: the **Team** tier now starts at $24 a seat, and annual plans get two months free. Happy to join the Acme call if their CFO wants the numbers walked through.",
    author: { name: "Alex Novak" },
    send: "draft",
  },
  {
    kind: "draft",
    id: "draft-email",
    destination: "email",
    to: "sarah.lin@accel.com",
    subject: "Dinner tonight at Nopa",
    body: "Hi Sarah,\n\nYes, 7:30 at Nopa still works. The batch session ends at 6, so I'll come straight from Mission Bay.\n\nSee you there,\nAlex",
    author: { name: "Alex Novak" },
    send: "draft",
  },
  {
    kind: "draft",
    id: "draft-linkedin",
    destination: "linkedin",
    body: "Excited to share that we've joined Y Combinator's winter batch. 🚀\n\nWe're building the browser where an agent does the work with you, on a canvas you can watch and steer. If you're in SF this spring, come say hi.",
    author: { name: "Alex Novak", handle: "Founder at Zephium" },
    send: "draft",
  },
  {
    kind: "draft",
    id: "draft-github",
    destination: "github",
    to: "acme/app#123",
    body: "Fixed in #486: expiry is now compared in UTC, so invites no longer lapse early for teams east of London. Added `expires_after_a_week_in_any_timezone` to cover it.",
    author: { name: "alex-novak" },
    send: "draft",
  },
];

const photo: MediaView = {
  kind: "media",
  id: "photo",
  media: "image",
  title: "Tofu and egg rice bowl",
  url: "https://www.justonecookbook.com/simmered-tofu-and-egg-rice-bowl/",
  picture: picture("39d6ebc6c7.jpg", 1600, 2400),
};

const lecture: MediaView = {
  kind: "media",
  id: "lecture",
  media: "video",
  title: "Compilers: lexical analysis",
  url: "https://www.youtube.com/watch?v=FgzyLoSkL5k",
  provider: "youtube",
  poster: picture("479469f204.jpg", 480, 360),
  duration: "17:42",
};

const memo: MediaView = {
  kind: "media",
  id: "memo",
  media: "audio",
  title: "Voice memo · pricing thoughts",
  url: "file:///Users/alex/Voice%20Memos/pricing.m4a",
  provider: "file",
  duration: "2:14",
};

const docsPage: PageObjectView = {
  kind: "page",
  id: "docs-page",
  url: "https://developers.cloudflare.com/r2/",
  title: "Cloudflare R2 · Object storage without egress fees",
  frame: `${LOOK}/fb56387659.png`,
};

const failedPage: PageObjectView = {
  kind: "page",
  id: "failed-page",
  url: "https://www.ycombinator.com/people",
  title: "People at Y Combinator",
};

const note: NoteView = {
  kind: "note",
  id: "note",
  markdown:
    "# Questions for the host\n\nIs the desk big enough for **two monitors**? Ask about parking for a rental car in March.",
};

const files: FileView[] = [
  {
    kind: "file",
    id: "file-image",
    name: "rice-bowl.jpg",
    file: "image",
    kindLabel: "JPEG image",
    size: 412_000,
    picture: picture("39d6ebc6c7.jpg", 1600, 2400),
  },
  {
    kind: "file",
    id: "file-text",
    name: "pricing-notes.md",
    file: "text",
    kindLabel: "Markdown",
    size: 3_200,
    lines:
      "# Pricing\n\nTeam: $24 a seat, billed yearly.\nStarter stays free up to 3 people.\n\n## Open questions\n- Usage-based model calls?\n- Education discount",
  },
  {
    kind: "file",
    id: "file-code",
    name: "token.rs",
    file: "code",
    language: "rust",
    kindLabel: "Rust source",
    size: 5_400,
    lines: excerpt.text,
  },
  {
    kind: "file",
    id: "file-other",
    name: "Q3 board update.key",
    file: "other",
    kindLabel: "Keynote presentation",
    size: 18_400_000,
  },
];

const folder: FolderView = {
  kind: "folder",
  id: "folder",
  name: "YC application",
  path: "~/Documents/YC application",
  count: 14,
  entries: [
    { name: "Pitch deck.pdf", folder: false },
    { name: "Founder video.mov", folder: false },
    { name: "Financials", folder: true },
    { name: "rice-bowl.jpg", folder: false, picture: picture("39d6ebc6c7.jpg", 1600, 2400) },
  ],
};

/** Every object, in the order the look test lays them out. */
export const allObjects: readonly ObjectView[] = [
  replySmall,
  replyFigures,
  stays,
  flights,
  products,
  videos,
  trip,
  today,
  providers,
  costs,
  growth,
  architecture,
  excerpt,
  fix,
  guide,
  ...drafts,
  photo,
  lecture,
  memo,
  docsPage,
  failedPage,
  note,
  ...files,
  folder,
];

const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun"];
const plot = (id: string, title: string, spec: PlotView["spec"]): PlotView => ({
  kind: "plot",
  id,
  title,
  spec,
});
const money = { format: "money", currency: "USD" } as const;
const quotes = [
  { x: "Hetzner", y: 64 },
  { x: "Cloudflare", y: 190 },
  { x: "Vercel", y: 420 },
  { x: "AWS", y: 780 },
];
/** One chart per plot style, with the data each style is for. */
const plots: PlotView[] = [
  plot("plot-bar", "Hosting per month", {
    kind: "bars",
    y: { label: "Cost", ...money },
    headline: { label: "Hetzner is the cheapest", value: "$64 / mo" },
    basis: "Smallest production tier with 4 vCPU, list prices on 28 Sep.",
    series: [{ name: "Cost", points: quotes }],
  }),
  plot("plot-bar-horizontal", "Time to first byte", {
    kind: "bars",
    horizontal: true,
    y: { label: "Latency", unit: "ms" },
    headline: { label: "Median from Warsaw", value: "38 ms" },
    series: [
      {
        name: "Latency",
        points: [
          { x: "Cloudflare Workers", y: 38 },
          { x: "Vercel Edge", y: 52 },
          { x: "AWS Lambda eu-central-1", y: 91 },
          { x: "Hetzner Falkenstein", y: 44 },
        ],
      },
    ],
  }),
  plot("plot-bar-stacked", "Monthly bill by service", {
    kind: "stacked",
    y: { label: "Cost", ...money },
    headline: { label: "June total", value: "$2,310" },
    series: [
      { name: "Model API", points: months.map((x, i) => ({ x, y: 600 + i * 150 })) },
      { name: "Database", points: months.map((x, i) => ({ x, y: 320 + i * 20 })) },
      { name: "Hosting", points: months.map((x, i) => ({ x, y: 180 + i * 16 })) },
    ],
  }),
  plot("plot-bar-grouped", "Signups by channel", {
    kind: "bars",
    y: { label: "Signups" },
    headline: { label: "Organic grew fastest", value: "+64%" },
    series: [
      { name: "Organic", points: months.map((x, i) => ({ x, y: 120 + i * 26 })) },
      { name: "Paid", points: months.map((x, i) => ({ x, y: 160 + (i % 3) * 18 })) },
    ],
  }),
  plot("plot-line", "Weekly active teams", {
    kind: "line",
    y: { label: "Teams" },
    headline: { label: "Up from 212 in January", value: "486" },
    series: [
      {
        name: "Teams",
        points: [212, 240, 268, 310, 402, 486].map((y, i) => ({ x: months[i]!, y })),
      },
    ],
  }),
  plot("plot-area", "Model spend as usage grows", {
    kind: "area",
    y: { label: "Spend", ...money },
    headline: { label: "By June", value: "$3,310" },
    basis: "At $3 per million input tokens and 40% cache hits.",
    series: [
      { name: "Model API", points: months.map((x, i) => ({ x, y: Math.round(820 * 1.32 ** i) })) },
    ],
  }),
  plot("plot-area-stacked", "Requests by region", {
    kind: "area",
    stack: true,
    y: { label: "Requests", unit: "M" },
    headline: { label: "Requests in June", value: "18.2M" },
    series: [
      { name: "Europe", points: months.map((x, i) => ({ x, y: 4 + i * 1.1 })) },
      { name: "North America", points: months.map((x, i) => ({ x, y: 3 + i * 0.9 })) },
      { name: "Asia", points: months.map((x, i) => ({ x, y: 1 + i * 0.5 })) },
    ],
  }),
  costs,
  plot("plot-radial", "Launch readiness", {
    kind: "radial",
    y: { format: "percent" },
    headline: { label: "Ready overall", value: "72%" },
    series: [
      {
        name: "Done",
        points: [
          { x: "Security review", y: 90 },
          { x: "Billing", y: 75 },
          { x: "Docs", y: 60 },
          { x: "Load tests", y: 40 },
        ],
      },
    ],
  }),
  plot("plot-radar", "How the providers compare", {
    kind: "radar",
    y: { label: "Score", max: 5 },
    series: [
      {
        name: "AWS",
        points: [
          { x: "Managed services", y: 5 },
          { x: "Price", y: 2 },
          { x: "Simplicity", y: 2 },
          { x: "Edge", y: 3 },
          { x: "Portability", y: 3 },
        ],
      },
      {
        name: "Cloudflare",
        points: [
          { x: "Managed services", y: 3 },
          { x: "Price", y: 4 },
          { x: "Simplicity", y: 4 },
          { x: "Edge", y: 5 },
          { x: "Portability", y: 2 },
        ],
      },
    ],
  }),
  plot("plot-range", "Rent near YC for a month", {
    kind: "range",
    y: { label: "Rent", ...money },
    headline: { label: "A furnished studio", value: "$3.2–4.8K" },
    series: [
      {
        name: "Rent",
        points: [
          { x: "Dogpatch", y: 3400, y2: 6900 },
          { x: "Mission Bay", y: 3200, y2: 4800 },
          { x: "SoMa", y: 2600, y2: 4400 },
          { x: "Mission", y: 2200, y2: 3900 },
        ],
      },
    ],
  }),
];

/** What the look test draws: one sheet per group, each object at its width. */
export const looks: Record<string, { object: ObjectView; width: number }[]> = {
  "charts-bars": plots.slice(0, 4).map((object) => ({ object, width: 520 })),
  "charts-trends": plots.slice(4, 7).map((object) => ({ object, width: 520 })),
  "charts-round": plots.slice(7).map((object) => ({ object, width: 520 })),
};
