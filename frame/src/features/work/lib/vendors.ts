/**
 * Well-known products and services and the host whose icon stands for them,
 * so a part the model named without a vendor still shows its mark. Closed:
 * a name missing here keeps its kind's glyph.
 */
const VENDORS: Record<string, readonly string[]> = {
  "redis.io": ["Redis", "Valkey"],
  "postgresql.org": ["PostgreSQL", "Postgres"],
  "mysql.com": ["MySQL"],
  "mongodb.com": ["MongoDB", "Mongo"],
  "aws.amazon.com": [
    "AWS",
    "Amazon Web Services",
    "S3",
    "EC2",
    "Lambda",
    "AWS Lambda",
    "DynamoDB",
    "SQS",
    "SNS",
    "RDS",
    "CloudFront",
    "Aurora",
  ],
  "cloud.google.com": ["Google Cloud", "GCP", "BigQuery", "Cloud Run", "Pub/Sub"],
  "azure.microsoft.com": ["Azure", "Microsoft Azure"],
  "nodejs.org": ["Node.js", "NodeJS"],
  "react.dev": ["React", "React Native"],
  "nextjs.org": ["Next.js", "NextJS"],
  "svelte.dev": ["Svelte", "SvelteKit"],
  "vuejs.org": ["Vue", "Vue.js", "Nuxt"],
  "angular.dev": ["Angular"],
  "djangoproject.com": ["Django"],
  "rubyonrails.org": ["Ruby on Rails", "Rails"],
  "laravel.com": ["Laravel"],
  "spring.io": ["Spring Boot", "Spring"],
  "kafka.apache.org": ["Kafka", "Apache Kafka"],
  "rabbitmq.com": ["RabbitMQ"],
  "nginx.org": ["Nginx"],
  "cloudflare.com": ["Cloudflare", "Cloudflare Workers", "R2"],
  "vercel.com": ["Vercel"],
  "netlify.com": ["Netlify"],
  "stripe.com": ["Stripe"],
  "twilio.com": ["Twilio"],
  "sendgrid.com": ["SendGrid"],
  "auth0.com": ["Auth0"],
  "okta.com": ["Okta"],
  "elastic.co": ["Elasticsearch", "Elastic", "Kibana", "Logstash"],
  "kubernetes.io": ["Kubernetes", "K8s"],
  "docker.com": ["Docker"],
  "terraform.io": ["Terraform"],
  "github.com": ["GitHub", "GitHub Actions"],
  "gitlab.com": ["GitLab"],
  "sentry.io": ["Sentry"],
  "datadoghq.com": ["Datadog"],
  "grafana.com": ["Grafana", "Loki"],
  "prometheus.io": ["Prometheus"],
  "openai.com": ["OpenAI", "ChatGPT"],
  "anthropic.com": ["Anthropic", "Claude"],
  "huggingface.co": ["Hugging Face", "HuggingFace"],
  "supabase.com": ["Supabase"],
  "firebase.google.com": ["Firebase", "Firestore"],
  "planetscale.com": ["PlanetScale"],
  "neon.tech": ["Neon"],
  "snowflake.com": ["Snowflake"],
  "sqlite.org": ["SQLite"],
  "rust-lang.org": ["Rust"],
  "python.org": ["Python"],
  "go.dev": ["Go", "Golang"],
  "typescriptlang.org": ["TypeScript"],
  "deno.com": ["Deno"],
  "bun.sh": ["Bun"],
  "tauri.app": ["Tauri"],
  "electronjs.org": ["Electron"],
  "flutter.dev": ["Flutter"],
  "swift.org": ["Swift"],
  "kotlinlang.org": ["Kotlin"],
  "java.com": ["Java"],
  "dotnet.microsoft.com": [".NET", "ASP.NET"],
  "php.net": ["PHP"],
  "graphql.org": ["GraphQL"],
  "apollographql.com": ["Apollo", "Apollo Server", "Apollo Client"],
  "prisma.io": ["Prisma"],
  "hasura.io": ["Hasura"],
  "clickhouse.com": ["ClickHouse"],
  "cassandra.apache.org": ["Cassandra", "Apache Cassandra"],
  "memcached.org": ["Memcached"],
  "minio.io": ["MinIO"],
  "backblaze.com": ["Backblaze", "Backblaze B2"],
  "digitalocean.com": ["DigitalOcean"],
  "hetzner.com": ["Hetzner"],
  "fly.io": ["Fly.io"],
  "render.com": ["Render"],
  "railway.app": ["Railway"],
  "heroku.com": ["Heroku"],
  "linode.com": ["Linode", "Akamai Cloud"],
  "ovhcloud.com": ["OVHcloud", "OVH"],
  "algolia.com": ["Algolia"],
  "meilisearch.com": ["Meilisearch"],
  "mapbox.com": ["Mapbox"],
  "segment.com": ["Segment"],
  "mixpanel.com": ["Mixpanel"],
  "amplitude.com": ["Amplitude"],
  "posthog.com": ["PostHog"],
  "resend.com": ["Resend"],
  "postmarkapp.com": ["Postmark"],
  "mailgun.com": ["Mailgun"],
  "slack.com": ["Slack"],
  "discord.com": ["Discord"],
  "notion.so": ["Notion"],
  "figma.com": ["Figma"],
  "linear.app": ["Linear"],
  "jira.atlassian.com": ["Jira"],
};

/** Names that are also plain words count only as a part's whole name. */
const PLAIN = new Set([
  "go",
  "swift",
  "spring",
  "rails",
  "render",
  "railway",
  "linear",
  "segment",
  "neon",
  "bun",
  "elastic",
  "apollo",
  "snowflake",
  "notion",
  "slack",
  "resend",
  "amplitude",
  "aurora",
  "loki",
]);
/** Names a note uses in passing ("react to", "a lambda") count only in a part's name. */
const NAMED = new Set(["react", "lambda", "electron", "stripe", "claude"]);

const HOSTS = new Map<string, string>();
for (const [host, names] of Object.entries(VENDORS))
  for (const name of names) HOSTS.set(name.toLowerCase(), host);
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\/]/gu, "\\$&");
// Whole words, longest first, so "Apache Kafka" wins over "Kafka" where both would match.
const words = (skip: (name: string) => boolean) =>
  new RegExp(
    `(?<![\\p{L}\\p{N}])(${[...HOSTS.keys()]
      .filter((name) => !skip(name))
      .sort((a, b) => b.length - a.length)
      .map(escape)
      .join("|")})(?![\\p{L}\\p{N}])`,
    "iu",
  );
const IN_NAME = words((name) => PLAIN.has(name));
const IN_NOTE = words((name) => PLAIN.has(name) || NAMED.has(name));

/** The host a part's name or note names, or none. */
function named(text: string, name: boolean): string | null {
  const value = text.trim().toLowerCase();
  if (!value) return null;
  if (name && PLAIN.has(value)) return HOSTS.get(value) ?? null;
  const match = (name ? IN_NAME : IN_NOTE).exec(value);
  return match ? (HOSTS.get(match[1]!) ?? null) : null;
}

/**
 * The host whose icon a diagram part shows: its stated vendor, otherwise a
 * well-known product its name, then its note, names as a whole word.
 */
export function vendorHost(vendor: string | undefined, name: string, note = ""): string | null {
  const stated = vendor?.trim().toLowerCase();
  if (stated) return stated;
  return named(name, true) ?? named(note, false);
}
