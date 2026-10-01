<script lang="ts">
  import Chart from "../Chart.svelte";
  import type { ChartSpec } from "../chart";

  const stages = ["Prototype", "Launch · 1k MAU", "Growth · 10k MAU", "Scale · 100k MAU"];
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun"];
  const specs: [string, ChartSpec][] = [
    [
      "Monthly operating-cost bands",
      {
        kind: "range",
        x: { label: "Stage", kind: "category" },
        y: { label: "USD / month", format: "money", currency: "USD" },
        series: [
          {
            name: "Monthly range",
            points: stages.map((x, index) => ({
              x,
              y: [100, 300, 1000, 5000][index]!,
              y2: [400, 1200, 4500, 20000][index]!,
            })),
          },
        ],
        basis: "Budget estimates, not quotes: cloud + ops + AI.",
      },
    ],
    [
      "Illustrative monthly cost breakdown",
      {
        kind: "bars",
        x: { label: "Cost category" },
        y: { label: "Estimated monthly cost", format: "money", currency: "USD" },
        series: [
          {
            name: "Estimated monthly USD",
            points: [
              ["Model API usage", 800],
              ["Application compute", 375],
              ["Database and cache", 300],
              ["Object storage and backups", 85],
              ["Monitoring and logging", 175],
              ["Network and edge", 112.5],
              ["Identity and product SaaS", 150],
            ].map(([x, y]) => ({ x: x as string, y: y as number })),
          },
        ],
      },
    ],
    [
      "Visitors by device",
      {
        kind: "bars",
        x: { label: "Month" },
        y: { label: "Visitors" },
        series: [
          {
            name: "Desktop",
            points: months.map((x, i) => ({ x, y: [186, 305, 237, 73, 209, 214][i]! })),
          },
          {
            name: "Mobile",
            points: months.map((x, i) => ({ x, y: [80, 200, 120, 190, 130, 140][i]! })),
          },
        ],
      },
    ],
    [
      "Visitors, stacked",
      {
        kind: "stacked",
        x: { label: "Month" },
        y: { label: "Visitors" },
        series: [
          {
            name: "Desktop",
            points: months.map((x, i) => ({ x, y: [186, 305, 237, 73, 209, 214][i]! })),
          },
          {
            name: "Mobile",
            points: months.map((x, i) => ({ x, y: [80, 200, 120, 190, 130, 140][i]! })),
          },
        ],
      },
    ],
    [
      "Visitors over time",
      {
        kind: "line",
        x: { label: "Month" },
        y: { label: "Visitors" },
        series: [
          {
            name: "Desktop",
            points: months.map((x, i) => ({ x, y: [186, 305, 237, 73, 209, 214][i]! })),
          },
          {
            name: "Mobile",
            points: months.map((x, i) => ({ x, y: [80, 200, 120, 190, 130, 140][i]! })),
          },
        ],
      },
    ],
    [
      "Visitors, area",
      {
        kind: "area",
        stack: true,
        x: { label: "Month" },
        y: { label: "Visitors" },
        series: [
          {
            name: "Desktop",
            points: months.map((x, i) => ({ x, y: [186, 305, 237, 73, 209, 214][i]! })),
          },
          {
            name: "Mobile",
            points: months.map((x, i) => ({ x, y: [80, 200, 120, 190, 130, 140][i]! })),
          },
        ],
      },
    ],
    [
      "Browser share",
      {
        kind: "donut",
        y: { label: "Visitors" },
        series: [
          {
            name: "Visitors",
            points: ["Chrome", "Safari", "Firefox", "Edge", "Other"].map((x, i) => ({
              x,
              y: [275, 200, 187, 173, 90][i]!,
            })),
          },
        ],
      },
    ],
  ];
</script>

<div class="sheet">
  {#each specs as [title, spec] (title)}
    <section class="card">
      <h3>{title}</h3>
      <Chart {title} {spec} />
    </section>
  {/each}
</div>

<style>
  .sheet {
    display: grid;
    grid-template-columns: repeat(2, 560px);
    gap: 24px;
    padding: 24px;
    background: var(--color-canvas);
    color: var(--color-text);
  }

  .card {
    padding: 20px 22px 18px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
  }

  h3 {
    margin: 0 0 14px;
    font-size: var(--text-body);
    font-weight: 600;
  }
</style>
