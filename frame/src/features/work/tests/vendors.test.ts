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

test("the stated vendor wins; a note names only when the name does not", () => {
  expect(vendorHost("PlanetScale.com ", "MySQL")).toBe("planetscale.com");
  expect(vendorHost(undefined, "Primary database", "Postgres 16 with replicas")).toBe(
    "postgresql.org",
  );
  expect(vendorHost(undefined, "Redis", "fronts Postgres")).toBe("redis.io");
});

test("plain words find nothing", () => {
  expect(vendorHost(undefined, "Cache")).toBeNull();
  expect(vendorHost(undefined, "Heap", "the notion of ownership")).toBeNull();
  expect(vendorHost(undefined, "Go to checkout")).toBeNull();
  expect(vendorHost(undefined, "Event handler", "react to changes")).toBeNull();
  // A part of a word is not the word.
  expect(vendorHost(undefined, "Rusty pipeline", "Gocardless")).toBeNull();
});
