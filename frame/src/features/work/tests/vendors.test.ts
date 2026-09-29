import { expect, test } from "vitest";
import { vendorHost } from "../lib/vendors";

test("a well-known product named by a part finds its host", () => {
  expect(vendorHost(undefined, "Redis")).toBe("redis.io");
  expect(vendorHost(undefined, "Apache Kafka cluster")).toBe("kafka.apache.org");
  expect(vendorHost(undefined, "Next.js frontend")).toBe("nextjs.org");
  expect(vendorHost(undefined, "stripe")).toBe("stripe.com");
  expect(vendorHost(undefined, "Go")).toBe("go.dev");
});

test("common aliases find the same host", () => {
  expect(vendorHost(undefined, "Postgres primary")).toBe("postgresql.org");
  expect(vendorHost(undefined, "K8s cluster")).toBe("kubernetes.io");
  expect(vendorHost(undefined, "S3 bucket")).toBe("aws.amazon.com");
  expect(vendorHost(undefined, "Lambda functions")).toBe("aws.amazon.com");
  expect(vendorHost(undefined, "Mongo")).toBe("mongodb.com");
  expect(vendorHost(undefined, "Analytics", "BigQuery on GCP")).toBe("cloud.google.com");
});

test("a stated vendor counts when the part names it; a note names only when the name does not", () => {
  expect(vendorHost("PlanetScale.com ", "PlanetScale MySQL")).toBe("planetscale.com");
  expect(vendorHost("aws.amazon.com", "Amazon Cognito")).toBe("aws.amazon.com");
  expect(vendorHost("aws.amazon.com", "Queue", "SQS with a dead-letter queue")).toBe(
    "aws.amazon.com",
  );
  // Run on a platform is not made by it; built with a framework its note names, it is.
  expect(vendorHost("aws.amazon.com", "API service", "Tenant checks")).toBeNull();
  expect(vendorHost("aws.amazon.com", "API service", "FastAPI; tenant checks")).toBe(
    "fastapi.tiangolo.com",
  );
  expect(vendorHost("PlanetScale.com", "MySQL")).toBe("mysql.com");
  expect(vendorHost(undefined, "Primary database", "Postgres 16 with replicas")).toBe(
    "postgresql.org",
  );
  expect(vendorHost(undefined, "Redis", "fronts Postgres")).toBe("redis.io");
});

test("a code host stands for a part only when the part is that host", () => {
  expect(vendorHost("github.com", "PostgreSQL pgvector")).toBe("postgresql.org");
  expect(vendorHost("github.com", "Vector search")).toBeNull();
  expect(vendorHost("github.com", "GitHub Actions")).toBe("github.com");
});

test("plain words find nothing", () => {
  expect(vendorHost(undefined, "Cache")).toBeNull();
  expect(vendorHost(undefined, "Heap", "the notion of ownership")).toBeNull();
  expect(vendorHost(undefined, "Go to checkout")).toBeNull();
  expect(vendorHost(undefined, "Event handler", "react to changes")).toBeNull();
  // A part of a word is not the word.
  expect(vendorHost(undefined, "Rusty pipeline", "Gocardless")).toBeNull();
});

test("languages, frameworks and runtimes a system is built from find their hosts", () => {
  expect(vendorHost(undefined, "Python API")).toBe("python.org");
  expect(vendorHost("temporal.io", "Temporal")).toBe("temporal.io");
  expect(vendorHost(undefined, "Celery workers")).toBe("celeryq.dev");
  expect(vendorHost(undefined, "Kubernetes")).toBe("kubernetes.io");
  expect(vendorHost(undefined, "RabbitMQ")).toBe("rabbitmq.com");
  expect(vendorHost(undefined, "Node")).toBe("nodejs.org");
  // A plain word counts only as a part's whole name.
  expect(vendorHost(undefined, "Node pool")).toBeNull();
  expect(vendorHost("temporal.io", "Job Orchestrator", "Durable workflows and retries")).toBeNull();
});
