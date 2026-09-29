import type { IconSvgElement } from "@hugeicons/svelte";
import Activity01Icon from "@hugeicons/core-free-icons/Activity01Icon";
import AiBrain01Icon from "@hugeicons/core-free-icons/AiBrain01Icon";
import AiChipIcon from "@hugeicons/core-free-icons/AiChipIcon";
import AiNetworkIcon from "@hugeicons/core-free-icons/AiNetworkIcon";
import Analytics01Icon from "@hugeicons/core-free-icons/Analytics01Icon";
import ApiGatewayIcon from "@hugeicons/core-free-icons/ApiGatewayIcon";
import ApiIcon from "@hugeicons/core-free-icons/ApiIcon";
import BrowserIcon from "@hugeicons/core-free-icons/BrowserIcon";
import BucketIcon from "@hugeicons/core-free-icons/BucketIcon";
import CloudIcon from "@hugeicons/core-free-icons/CloudIcon";
import CodeIcon from "@hugeicons/core-free-icons/CodeIcon";
import CommandLineIcon from "@hugeicons/core-free-icons/CommandLineIcon";
import ComputerIcon from "@hugeicons/core-free-icons/ComputerIcon";
import ContainerIcon from "@hugeicons/core-free-icons/ContainerIcon";
import CpuIcon from "@hugeicons/core-free-icons/CpuIcon";
import CreditCardIcon from "@hugeicons/core-free-icons/CreditCardIcon";
import CubeIcon from "@hugeicons/core-free-icons/CubeIcon";
import DatabaseIcon from "@hugeicons/core-free-icons/DatabaseIcon";
import DatabaseSearchIcon from "@hugeicons/core-free-icons/DatabaseSearchIcon";
import DatabaseZapIcon from "@hugeicons/core-free-icons/DatabaseZapIcon";
import File01Icon from "@hugeicons/core-free-icons/File01Icon";
import GitBranchIcon from "@hugeicons/core-free-icons/GitBranchIcon";
import Globe02Icon from "@hugeicons/core-free-icons/Globe02Icon";
import HardDriveIcon from "@hugeicons/core-free-icons/HardDriveIcon";
import Key01Icon from "@hugeicons/core-free-icons/Key01Icon";
import Mail01Icon from "@hugeicons/core-free-icons/Mail01Icon";
import Message01Icon from "@hugeicons/core-free-icons/Message01Icon";
import Notification01Icon from "@hugeicons/core-free-icons/Notification01Icon";
import PackageIcon from "@hugeicons/core-free-icons/PackageIcon";
import Plug01Icon from "@hugeicons/core-free-icons/Plug01Icon";
import Queue01Icon from "@hugeicons/core-free-icons/Queue01Icon";
import Robot01Icon from "@hugeicons/core-free-icons/Robot01Icon";
import Search01Icon from "@hugeicons/core-free-icons/Search01Icon";
import ServerStack01Icon from "@hugeicons/core-free-icons/ServerStack01Icon";
import Settings02Icon from "@hugeicons/core-free-icons/Settings02Icon";
import Share08Icon from "@hugeicons/core-free-icons/Share08Icon";
import Shield01Icon from "@hugeicons/core-free-icons/Shield01Icon";
import ShieldKeyIcon from "@hugeicons/core-free-icons/ShieldKeyIcon";
import SmartPhone01Icon from "@hugeicons/core-free-icons/SmartPhone01Icon";
import Task01Icon from "@hugeicons/core-free-icons/Task01Icon";
import TimeScheduleIcon from "@hugeicons/core-free-icons/TimeScheduleIcon";
import UserGroupIcon from "@hugeicons/core-free-icons/UserGroupIcon";
import UserIcon from "@hugeicons/core-free-icons/UserIcon";
import WebhookIcon from "@hugeicons/core-free-icons/WebhookIcon";
import WorkflowSquare03Icon from "@hugeicons/core-free-icons/WorkflowSquare03Icon";

/**
 * What a part does, read from its words: the more particular role first, so a
 * "vector store" is a vector store before it is a store and "Auth API" is
 * auth before it is an API. Whole words only.
 */
const ROLES: readonly (readonly [RegExp, IconSvgElement])[] = [
  [/\b(users|customers|people|visitors|end users|audience)\b/u, UserGroupIcon],
  [/\b(person|user|customer|admin|operator|developer)\b/u, UserIcon],
  [/\b(mobile|ios|android|iphone|react native|expo|phone)\b/u, SmartPhone01Icon],
  [/\b(browser|web ?app|web client|frontend|front-end|spa|website|web ui)\b/u, BrowserIcon],
  [/\b(desktop|laptop|mac ?app|electron)\b/u, ComputerIcon],
  [/\b(cli|terminal|shell|command line)\b/u, CommandLineIcon],
  [/\b(cdn|edge|dns|cloudfront)\b/u, Globe02Icon],
  [/\b(waf|firewall|security|guardrails?|policy|policies|sandbox)\b/u, Shield01Icon],
  [/\b(billing|payments?|metering|subscriptions?|checkout|invoices?)\b/u, CreditCardIcon],
  [
    /\b(auth|authn|authz|authentication|authorization|identity|sso|login|sign[ -]?in|oauth|iam|sessions?)\b/u,
    ShieldKeyIcon,
  ],
  [/\b(secrets?|keys?|vault|kms)\b/u, Key01Icon],
  [/\b(e-?mail|mail|smtp)\b/u, Mail01Icon],
  [/\b(notifications?|push|sms|alerts?)\b/u, Notification01Icon],
  [/\b(analytics|metrics|telemetry|product events|bi)\b/u, Analytics01Icon],
  [/\b(monitoring|observability|logs?|logging|traces?|tracing|apm)\b/u, Activity01Icon],
  [/\b(load ?balancers?|ingress|reverse proxy|proxy)\b/u, Share08Icon],
  [/\b(gateway|router|routing)\b/u, ApiGatewayIcon],
  [
    /\b(vector|vectors|embeddings? (store|index|db)|pgvector|pinecone|weaviate|qdrant|milvus|chroma)\b/u,
    DatabaseSearchIcon,
  ],
  [/\b(embed|embeds|embedding|embeddings|rerank|reranker|reranking)\b/u, AiNetworkIcon],
  [
    /\b(orchestrator|orchestration|workflows?|pipelines?|step functions?|dag)\b/u,
    WorkflowSquare03Icon,
  ],
  [/\b(scheduler|cron|timer|scheduled)\b/u, TimeScheduleIcon],
  [/\b(queues?|streams?|bus|topics?|pub\/?sub|broker)\b/u, Queue01Icon],
  [/\b(jobs?|tasks?|batch)\b/u, Task01Icon],
  [/\b(agents?|assistant|copilot|bot)\b/u, Robot01Icon],
  [/\b(workers?|executors?|runners?|compute|functions?|lambdas?)\b/u, CpuIcon],
  [/\b(llms?|models?|inference|ai|ml)\b/u, AiBrain01Icon],
  [/\b(gpus?|accelerators?)\b/u, AiChipIcon],
  [/\b(cache|caching|kv|key-value)\b/u, DatabaseZapIcon],
  [/\b(search|index)\b/u, Search01Icon],
  [/\b(object storage|buckets?|s3|blob|blobs)\b/u, BucketIcon],
  [/\b(storage|files?|uploads?|disk|volumes?)\b/u, HardDriveIcon],
  [/\b(databases?|db|sql|warehouse|store|tables?|ledger)\b/u, DatabaseIcon],
  [/\b(webhooks?|callbacks?)\b/u, WebhookIcon],
  [/\b(containers?|pods?|cluster)\b/u, ContainerIcon],
  [/\b(registry|packages?|artifacts?)\b/u, PackageIcon],
  [/\b(git|repo|repos|repository|ci|ci\/cd)\b/u, GitBranchIcon],
  [/\b(chat|messages?|messaging)\b/u, Message01Icon],
  [/\b(documents?|pdfs?|docs)\b/u, File01Icon],
  [/\b(code|editor|ide|compiler|parser)\b/u, CodeIcon],
  [/\b(config|configuration|settings|feature flags?)\b/u, Settings02Icon],
  [/\b(plugins?|integrations?|connectors?|mcp|extensions?)\b/u, Plug01Icon],
  [/\b(api|apis|rest|graphql|grpc|trpc|endpoints?|backend|server)\b/u, ApiIcon],
];

/** A part's kind when its words say nothing more particular. */
const KINDS: Record<string, IconSvgElement> = {
  client: ComputerIcon,
  edge: Globe02Icon,
  gateway: ApiGatewayIcon,
  service: ServerStack01Icon,
  worker: CpuIcon,
  model: AiBrain01Icon,
  store: DatabaseIcon,
  queue: Queue01Icon,
  cache: DatabaseZapIcon,
  storage: HardDriveIcon,
  external: CloudIcon,
};

/**
 * The glyph a part shows when no product's mark stands for it: by its name
 * first, then its kind, then its note; a part that says nothing is a cube.
 */
export function roleGlyph(part: { name: string; kind: string; note?: string | null }) {
  const name = part.name.toLowerCase();
  for (const [words, glyph] of ROLES) if (words.test(name)) return glyph;
  const kind = KINDS[part.kind];
  if (kind) return kind;
  const note = (part.note ?? "").toLowerCase();
  for (const [words, glyph] of ROLES) if (words.test(note)) return glyph;
  return CubeIcon;
}
