use super::*;
fn paragraph() -> DocumentNode {
    DocumentNode {
        kind: "paragraph".into(),
        content: vec![],
        text: None,
        attrs: None,
        marks: vec![],
    }
}
#[test]
fn validates_real_calendar_dates_and_exact_revisions() {
    assert!(valid_date("2028-02-29"));
    assert!(!valid_date("2027-02-29"));
    assert!(!valid_date("2027-13-01"));
    assert_eq!(revision("9007199254740993"), Some(9007199254740993));
    assert_eq!(revision("01"), None);
}
#[test]
fn accepts_empty_paragraphs_but_rejects_executable_or_unknown_document_shapes() {
    let mut doc = NoteDocument {
        version: 1,
        document: DocumentNode {
            kind: "doc".into(),
            content: vec![paragraph()],
            text: None,
            attrs: None,
            marks: vec![],
        },
    };
    assert!(doc.validate());
    doc.document.content[0].kind = "script".into();
    assert!(!doc.validate());
}
#[test]
fn document_limits_bound_depth_and_node_count() {
    let mut doc = NoteDocument {
        version: 1,
        document: DocumentNode {
            kind: "doc".into(),
            content: vec![paragraph(); 4096],
            text: None,
            attrs: None,
            marks: vec![],
        },
    };
    assert!(!doc.validate());
    doc.document.content = vec![paragraph()];
    for _ in 0..18 {
        let child = doc.document.content.remove(0);
        doc.document.content.push(DocumentNode {
            kind: "blockquote".into(),
            content: vec![child],
            text: None,
            attrs: None,
            marks: vec![],
        });
    }
    assert!(!doc.validate());
}

/// Functional update does not reach inside an enum variant, so the parts a test
/// varies live in a struct and the variant is built from it.
#[derive(Default)]
struct TaskParts {
    completed: bool,
    due_date: Option<String>,
    due_time: Option<String>,
    status: TaskStatus,
    context: Option<TaskContext>,
    sort_key: Option<String>,
    work: Option<String>,
}
fn task(parts: TaskParts) -> ResourceDraft {
    ResourceDraft {
        title: "Review the evidence".into(),
        pinned: false,
        related: vec![],
        content: ResourceContent::Task {
            details: Default::default(),
            description: String::new(),
            completed: parts.completed,
            due_date: parts.due_date,
            due_time: parts.due_time,
            status: parts.status,
            assignee: TaskActor::User,
            origin: TaskActor::User,
            context: parts.context,
            sort_key: parts.sort_key,
            work: parts.work,
        },
    }
}
#[test]
fn task_status_and_completed_must_agree() {
    assert!(task(TaskParts::default()).validate());
    assert!(task(TaskParts {
        completed: true,
        status: TaskStatus::Done,
        ..TaskParts::default()
    })
    .validate());
    // Done-ness reached through only one of the two representations is exactly
    // the state the listing column and the drawn row would disagree about.
    assert!(!task(TaskParts {
        completed: true,
        ..TaskParts::default()
    })
    .validate());
    assert!(!task(TaskParts {
        status: TaskStatus::Done,
        ..TaskParts::default()
    })
    .validate());
    // An in-flight task is not a finished one.
    assert!(task(TaskParts {
        status: TaskStatus::Active,
        ..TaskParts::default()
    })
    .validate());
}
#[test]
fn task_context_accepts_only_a_url_the_browser_would_commit() {
    let with = |url: &str| {
        task(TaskParts {
            context: Some(TaskContext {
                url: url.into(),
                title: "Pricing".into(),
            }),
            ..TaskParts::default()
        })
        .validate()
    };
    assert!(with("https://example.com/pricing"));
    assert!(!with("javascript:alert(1)"));
    assert!(!with("file:///etc/passwd"));
    assert!(!with("https://user:secret@example.com/"));
    assert!(!with("not a url"));
}
#[test]
fn task_ordering_and_reserved_binding_stay_bounded() {
    let sorted = |key: &str| {
        task(TaskParts {
            sort_key: Some(key.into()),
            ..TaskParts::default()
        })
        .validate()
    };
    assert!(sorted("m"));
    assert!(!sorted(""));
    assert!(!sorted("has space"));
    assert!(!sorted(&"k".repeat(65)));
    let bound = |value: &str| {
        task(TaskParts {
            work: Some(value.into()),
            ..TaskParts::default()
        })
        .validate()
    };
    assert!(bound("00000000000000000000000001"));
    assert!(!bound("not-a-ulid"));
}

#[test]
fn a_due_time_is_a_wall_clock_moment_on_a_day_it_has() {
    let at = |time: &str, date: Option<&str>| {
        task(TaskParts {
            due_date: date.map(Into::into),
            due_time: Some(time.into()),
            ..TaskParts::default()
        })
        .validate()
    };
    assert!(at("09:30", Some("2026-09-21")));
    assert!(at("00:00", Some("2026-09-21")));
    assert!(at("23:59", Some("2026-09-21")));
    assert!(!at("24:00", Some("2026-09-21")));
    assert!(!at("09:60", Some("2026-09-21")));
    assert!(!at("9:30", Some("2026-09-21")));
    assert!(!at("09:30:00", Some("2026-09-21")));
    // A time with no day is not a moment, and nothing could sort or show it.
    assert!(!at("09:30", None));
    // A day without a time stays perfectly ordinary.
    assert!(task(TaskParts {
        due_date: Some("2026-09-21".into()),
        ..TaskParts::default()
    })
    .validate());
}

#[test]
fn task_details_reject_ambiguous_membership_and_duplicate_subtasks() {
    let mut draft = task(TaskParts::default());
    let ResourceContent::Task { details, .. } = &mut draft.content else {
        panic!()
    };
    details.list = Some("00000000000000000000000001".into());
    details.inbox = true;
    assert!(!draft.validate());
    let ResourceContent::Task { details, .. } = &mut draft.content else {
        panic!()
    };
    details.inbox = false;
    let step = TaskStep {
        id: "step-00000000000001".into(),
        title: "Read the source".into(),
        completed: false,
    };
    details.steps = vec![step.clone(), step];
    assert!(!draft.validate());
    let ResourceContent::Task { details, .. } = &mut draft.content else {
        panic!()
    };
    details.steps.pop();
    assert!(draft.validate());
}
