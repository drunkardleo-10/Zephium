import type { WorkArtifactDataV1 } from "$domain/work";
import * as m from "$shared/i18n/messages";

type Field = {
  label: string;
  value: string | boolean;
  change: (value: string | boolean) => WorkArtifactDataV1;
};

/** Native semantic data to ordinary controls; no generated markup or field paths. */
export function artifactFields(data: WorkArtifactDataV1): Field[] {
  const fields: Field[] = [];
  function text(
    label: string,
    value: string,
    update: (draft: WorkArtifactDataV1, value: string) => void,
  ) {
    fields.push({
      label,
      value,
      change: (next) => {
        const draft = structuredClone(data);
        if (typeof next === "string") update(draft, next);
        return draft;
      },
    });
  }
  switch (data.kind) {
    case "document":
      data.paragraphs.forEach((value, i) =>
        text(m.work_paragraph({ number: i + 1 }), value, (draft, value) => {
          if (draft.kind === "document") draft.paragraphs[i] = value;
        }),
      );
      break;
    case "table":
      data.columns.forEach((value, i) =>
        text(m.work_column({ number: i + 1 }), value, (draft, value) => {
          if (draft.kind === "table") draft.columns[i] = value;
        }),
      );
      data.rows.forEach((row, r) =>
        row.forEach((value, c) =>
          text(
            m.work_cell({ row: r + 1, column: data.columns[c] ?? "" }),
            value,
            (draft, value) => {
              if (draft.kind === "table" && draft.rows[r]) draft.rows[r][c] = value;
            },
          ),
        ),
      );
      break;
    case "comparison":
      data.criteria.forEach((value, i) =>
        text(m.work_column({ number: i + 1 }), value, (draft, value) => {
          if (draft.kind === "comparison") draft.criteria[i] = value;
        }),
      );
      data.alternatives.forEach((row, r) => {
        text(m.work_alternative({ number: r + 1 }), row.name, (draft, value) => {
          if (draft.kind === "comparison" && draft.alternatives[r])
            draft.alternatives[r].name = value;
        });
        row.values.forEach((value, c) =>
          text(`${row.name} · ${data.criteria[c]}`, value, (draft, value) => {
            if (draft.kind === "comparison" && draft.alternatives[r])
              draft.alternatives[r].values[c] = value;
          }),
        );
      });
      break;
    case "chart":
      text(m.work_horizontal_axis(), data.x_label, (draft, value) => {
        if (draft.kind === "chart") draft.x_label = value;
      });
      text(m.work_vertical_axis(), data.y_label, (draft, value) => {
        if (draft.kind === "chart") draft.y_label = value;
      });
      data.series.forEach((series, s) => {
        text(m.work_series_number({ number: s + 1 }), series.name, (draft, value) => {
          if (draft.kind === "chart" && draft.series[s]) draft.series[s].name = value;
        });
        series.points.forEach((point, p) => {
          text(m.work_point_label({ number: p + 1 }), point.label, (draft, value) => {
            if (draft.kind === "chart" && draft.series[s]?.points[p])
              draft.series[s].points[p].label = value;
          });
          text(`${series.name} · ${point.label}`, point.value, (draft, value) => {
            if (draft.kind === "chart" && draft.series[s]?.points[p])
              draft.series[s].points[p].value = value;
          });
        });
      });
      break;
    case "checklist":
      data.items.forEach((item, i) => {
        text(m.work_item_number({ number: i + 1 }), item.text, (draft, value) => {
          if (draft.kind === "checklist" && draft.items[i]) draft.items[i].text = value;
        });
        fields.push({
          label: item.text,
          value: item.completed,
          change: (value) => {
            const draft = structuredClone(data);
            if (typeof value === "boolean" && draft.items[i]) draft.items[i].completed = value;
            return draft;
          },
        });
      });
      break;
    case "evidence_collection":
      text(m.work_summary(), data.summary, (draft, value) => {
        if (draft.kind === "evidence_collection") draft.summary = value;
      });
      break;
    case "browser_resource_preview":
      text(m.work_title(), data.title, (draft, value) => {
        if (draft.kind === "browser_resource_preview") draft.title = value;
      });
      text(m.work_location(), data.url, (draft, value) => {
        if (draft.kind === "browser_resource_preview") draft.url = value;
      });
      text(m.work_summary(), data.summary, (draft, value) => {
        if (draft.kind === "browser_resource_preview") draft.summary = value;
      });
  }
  return fields;
}
