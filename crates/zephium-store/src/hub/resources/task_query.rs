use super::*;
use serde::{Deserialize, Serialize};

// All ascending, including identity: the cursor retains the ordering values even
// if its task is subsequently deleted. It is bound to the entire query.
type Position = (i64, i64, i64, i64, String, String, String, String);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    view: TaskView,
    list: Option<String>,
    today: String,
    search: String,
    position: Position,
}

/// The day a task is next answerable for: its planned day or its deadline,
/// whichever comes first. Views, counts and sections all place a task by it.
const DAY_DUE: &str = "CASE WHEN task_deadline IS NULL THEN due_date \
    WHEN due_date IS NULL OR task_deadline<due_date THEN task_deadline ELSE due_date END";

fn counts(conn: &Connection, today: &str) -> rusqlite::Result<TaskCounts> {
    conn.query_row(
        &format!(
            "SELECT count(*) FILTER (WHERE trashed=0 AND completed=0 AND day_due<=?1),
            count(*) FILTER (WHERE trashed=0 AND completed=0 AND day_due<?1),
            count(*) FILTER (WHERE trashed=0 AND completed=0 AND day_due>?1),
            count(*) FILTER (WHERE trashed=0 AND completed=0),
            count(*) FILTER (WHERE trashed=0 AND completed=1),
            count(*) FILTER (WHERE trashed=1),
            count(*) FILTER (WHERE trashed=0 AND completed=0 AND task_inbox=1)
            FROM (SELECT trashed,completed,task_inbox,{DAY_DUE} AS day_due
                FROM user_resources WHERE kind='task')"
        ),
        [today],
        |row| {
            Ok(TaskCounts {
                inbox: row.get(6)?,
                today: row.get(0)?,
                overdue: row.get(1)?,
                upcoming: row.get(2)?,
                all: row.get(3)?,
                completed: row.get(4)?,
                trash: row.get(5)?,
            })
        },
    )
}

pub(super) fn overview(conn: &Connection, today: &str) -> rusqlite::Result<ResourceResponse> {
    Ok(ResourceResponse::TaskOverview {
        lists: task_lists::lists(conn)?,
        counts: counts(conn, today)?,
    })
}

pub(super) fn list(conn: &Connection, query: TaskQuery) -> rusqlite::Result<ResourceResponse> {
    let search = query.search.trim().to_lowercase();
    let cursor = match query.after.as_deref() {
        None => None,
        Some(value) => match serde_json::from_str::<Cursor>(value) {
            Ok(cursor)
                if cursor.view == query.view
                    && cursor.list == query.list
                    && cursor.today == query.today
                    && cursor.search == search =>
            {
                Some(cursor)
            }
            _ => return Ok(error(ResourceError::Invalid)),
        },
    };
    let counts = counts(conn, &query.today)?;
    let predicate = match query.view {
        TaskView::Inbox => "trashed=0 AND completed=0 AND task_inbox=1",
        TaskView::Today => "trashed=0 AND completed=0 AND day_due<=?1",
        TaskView::Upcoming => "trashed=0 AND completed=0 AND day_due>?1",
        TaskView::All => "trashed=0",
        TaskView::Completed => "trashed=0 AND completed=1",
        TaskView::Trash => "trashed=1",
    };
    let boundary = cursor.as_ref().map(|c| c.position.clone()).unwrap_or((
        -1,
        0,
        0,
        0,
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ));
    let sql = format!("WITH dated AS (
        SELECT *, {DAY_DUE} AS day_due FROM user_resources WHERE kind='task'
    ), ordered AS (
        SELECT {SUMMARY_COLUMNS},
        CASE WHEN completed=1 THEN 5 WHEN day_due IS NULL THEN 4
            WHEN day_due<?1 THEN 0 WHEN day_due=?1 THEN 1
            WHEN day_due=date(?1,'+1 day') THEN 2 ELSE 3 END AS section,
        CASE status WHEN 'blocked' THEN 0 WHEN 'active' THEN 1 WHEN 'open' THEN 2 ELSE 3 END AS rank,
        CASE WHEN completed=1 THEN 0 ELSE 1-pinned END AS unpinned, CASE WHEN completed=1 THEN 0 ELSE sort_key IS NULL END AS unplaced,
        CASE WHEN completed=1 THEN '' ELSE coalesce(sort_key,'') END AS position, CASE WHEN completed=1 THEN printf('%020d',9223372036854775807-coalesce(CAST(task_completed_at AS INTEGER),0)) ELSE coalesce(day_due,'9999-12-31') END AS day,
        coalesce(due_time,'24:00') AS clock, task_list,task_inbox,task_priority,task_steps,task_steps_done,task_completed_at,task_deadline,task_duration
        FROM dated WHERE ({predicate}) AND instr(search_text,?2)>0 AND (?12 IS NULL OR task_list=?12)
    ) SELECT {SUMMARY_COLUMNS},section,rank,unpinned,unplaced,position,day,clock,task_list,task_inbox,task_priority,task_steps,task_steps_done,task_completed_at,task_deadline,task_duration FROM ordered
    WHERE (section,rank,unpinned,unplaced,position,day,clock,id)>(?3,?4,?5,?6,?7,?8,?9,?10)
    ORDER BY section,rank,unpinned,unplaced,position,day,clock,id LIMIT ?11");
    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map(
            params![
                query.today,
                search,
                boundary.0,
                boundary.1,
                boundary.2,
                boundary.3,
                boundary.4,
                boundary.5,
                boundary.6,
                boundary.7,
                u32::from(query.limit) + 1,
                query.list
            ],
            |row| {
                let summary = summary_row(row)?;
                let position = (
                    row.get(15)?,
                    row.get(16)?,
                    row.get(17)?,
                    row.get(18)?,
                    row.get(19)?,
                    row.get(20)?,
                    row.get(21)?,
                    summary.id.clone(),
                );
                let metadata = TaskMetadata {
                    id: summary.id.clone(),
                    list: row.get(22)?,
                    inbox: row.get(23)?,
                    priority: match row.get::<_, String>(24)?.as_str() {
                        "high" => TaskPriority::High,
                        "medium" => TaskPriority::Medium,
                        "low" => TaskPriority::Low,
                        _ => TaskPriority::None,
                    },
                    steps: row.get(25)?,
                    steps_done: row.get(26)?,
                    completed_at: row.get(27)?,
                    deadline: row.get(28)?,
                    duration: row.get(29)?,
                };
                Ok((summary, position, metadata))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more = rows.len() > usize::from(query.limit);
    let mut rows = rows;
    rows.truncate(usize::from(query.limit));
    let next = if more {
        rows.last()
            .map(|(_, position, _)| {
                serde_json::to_string(&Cursor {
                    view: query.view,
                    list: query.list,
                    today: query.today,
                    search,
                    position: position.clone(),
                })
                .map_err(|_| rusqlite::Error::InvalidQuery)
            })
            .transpose()?
    } else {
        None
    };
    Ok(ResourceResponse::TaskPage {
        lists: task_lists::lists(conn)?,
        metadata: rows.iter().map(|(_, _, meta)| meta.clone()).collect(),
        items: rows.into_iter().map(|(item, _, _)| item).collect(),
        next,
        counts,
    })
}
