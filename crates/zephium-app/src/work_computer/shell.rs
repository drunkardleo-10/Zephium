//! What a finished command tells the agent: its outcome, test counts when it
//! ran tests, and the few lines that matter. The whole bounded output stays
//! with the step as a source the agent can cite, never in its context.
use regex::Regex;
use std::sync::OnceLock;

/// Lines of output shown to the agent, at most.
const SHOWN_LINES: usize = 40;
const SHOWN_BYTES: usize = 4 * 1024;
const MAX_FAILURES: usize = 8;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TestSummary {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    /// Failing test names, as the runner printed them.
    pub failures: Vec<String>,
}
impl TestSummary {
    pub fn line(&self) -> String {
        let mut parts = vec![format!("{} passed", self.passed)];
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        let mut line = parts.join(", ");
        if !self.failures.is_empty() {
            line.push_str(&format!(" ({})", self.failures.join(", ")));
        }
        line
    }
    fn is_empty(&self) -> bool {
        self.passed == 0 && self.failed == 0 && self.skipped == 0
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}
fn number(captures: &regex::Captures<'_>, name: &str) -> u32 {
    captures
        .name(name)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}
fn failure(summary: &mut TestSummary, name: &str) {
    let name = name.trim();
    if !name.is_empty()
        && summary.failures.len() < MAX_FAILURES
        && !summary.failures.iter().any(|seen| seen == name)
    {
        summary.failures.push(name.chars().take(120).collect());
    }
}

/// Test counts from the runners people use most: cargo, pytest, jest,
/// vitest, go, mocha and XCTest. None when the output shows no test run.
pub fn tests(output: &str) -> Option<TestSummary> {
    static CARGO: OnceLock<Regex> = OnceLock::new();
    static CARGO_FAIL: OnceLock<Regex> = OnceLock::new();
    static PYTEST: OnceLock<Regex> = OnceLock::new();
    static PYTEST_COUNT: OnceLock<Regex> = OnceLock::new();
    static PYTEST_FAIL: OnceLock<Regex> = OnceLock::new();
    static JEST: OnceLock<Regex> = OnceLock::new();
    static VITEST: OnceLock<Regex> = OnceLock::new();
    static JS_FAIL: OnceLock<Regex> = OnceLock::new();
    static GO: OnceLock<Regex> = OnceLock::new();
    static MOCHA: OnceLock<Regex> = OnceLock::new();
    static XCTEST: OnceLock<Regex> = OnceLock::new();
    static UNITTEST: OnceLock<Regex> = OnceLock::new();
    static UNITTEST_END: OnceLock<Regex> = OnceLock::new();
    static UNITTEST_FAIL: OnceLock<Regex> = OnceLock::new();
    let mut summary = TestSummary::default();
    let mut seen = false;

    let cargo = re(
        &CARGO,
        r"test result: \w+\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; (?P<ignored>\d+) ignored",
    );
    for c in cargo.captures_iter(output) {
        seen = true;
        summary.passed += number(&c, "passed");
        summary.failed += number(&c, "failed");
        summary.skipped += number(&c, "ignored");
    }
    if seen {
        for c in re(&CARGO_FAIL, r"(?m)^test (?P<name>\S+) \.\.\. FAILED").captures_iter(output) {
            failure(&mut summary, &c["name"]);
        }
        return Some(summary);
    }

    if let Some(line) = re(
        &PYTEST,
        r"(?m)^=+ (?P<body>.*\b(passed|failed|error|errors)\b.*) in [\d.]+s",
    )
    .captures_iter(output)
    .last()
    {
        for c in re(
            &PYTEST_COUNT,
            r"(?P<n>\d+) (?P<what>passed|failed|errors?|skipped|xfailed)",
        )
        .captures_iter(&line["body"])
        {
            let n: u32 = c["n"].parse().unwrap_or(0);
            match &c["what"] {
                "passed" => summary.passed += n,
                "skipped" | "xfailed" => summary.skipped += n,
                _ => summary.failed += n,
            }
        }
        for c in re(&PYTEST_FAIL, r"(?m)^(?:FAILED|ERROR) (?P<name>\S+)").captures_iter(output) {
            failure(&mut summary, &c["name"]);
        }
        return Some(summary);
    }

    let js = re(
        &JEST,
        r"(?m)^Tests:\s+(?:(?P<failed>\d+) failed, )?(?:(?P<skipped>\d+) skipped, )?(?:(?P<todo>\d+) todo, )?(?:(?P<passed>\d+) passed, )?\d+ total",
    );
    let vitest = re(
        &VITEST,
        r"(?m)^\s*Tests\s+(?:(?P<failed>\d+) failed)?(?:\s*\|\s*)?(?:(?P<passed>\d+) passed)?(?:\s*\|\s*)?(?:(?P<skipped>\d+) skipped)?",
    );
    for c in js.captures_iter(output).chain(vitest.captures_iter(output)) {
        let (p, f, s) = (
            number(&c, "passed"),
            number(&c, "failed"),
            number(&c, "skipped"),
        );
        if p + f + s > 0 {
            seen = true;
            summary.passed += p;
            summary.failed += f;
            summary.skipped += s;
        }
    }
    if seen {
        for c in re(&JS_FAIL, r"(?m)^\s*(?:●|FAIL|×|✗)\s+(?P<name>.+?)\s*$").captures_iter(output)
        {
            if !c["name"].starts_with("Test suite") {
                failure(&mut summary, &c["name"]);
            }
        }
        return Some(summary);
    }

    for c in re(&GO, r"(?m)^\s*--- (?P<what>PASS|FAIL|SKIP): (?P<name>\S+)").captures_iter(output) {
        seen = true;
        match &c["what"] {
            "PASS" => summary.passed += 1,
            "SKIP" => summary.skipped += 1,
            _ => {
                summary.failed += 1;
                failure(&mut summary, &c["name"]);
            }
        }
    }
    if seen {
        return Some(summary);
    }

    if let Some(c) = re(&MOCHA, r"(?m)^\s+(?P<passed>\d+) passing[^\n]*(?:\n\s+(?P<skipped>\d+) pending)?(?:\n\s+(?P<failed>\d+) failing)?").captures(output) {
        summary.passed = number(&c, "passed");
        summary.skipped = number(&c, "skipped");
        summary.failed = number(&c, "failed");
        return Some(summary);
    }

    if let Some(ran) = re(&UNITTEST, r"(?m)^Ran (?P<total>\d+) tests? in [\d.]+s")
        .captures_iter(output)
        .last()
    {
        let total = number(&ran, "total");
        if let Some(end) = re(
            &UNITTEST_END,
            r"(?m)^(?:OK|FAILED)(?: \((?:failures=(?P<failures>\d+))?(?:, )?(?:errors=(?P<errors>\d+))?(?:, )?(?:skipped=(?P<skipped>\d+))?[^)]*\))?\s*$",
        )
        .captures_iter(output)
        .last()
        {
            summary.failed = number(&end, "failures") + number(&end, "errors");
            summary.skipped = number(&end, "skipped");
            summary.passed = total.saturating_sub(summary.failed + summary.skipped);
            for c in re(&UNITTEST_FAIL, r"(?m)^(?:FAIL|ERROR): (?P<name>\S+)").captures_iter(output) {
                failure(&mut summary, &c["name"]);
            }
            return Some(summary);
        }
    }

    if let Some(c) = re(
        &XCTEST,
        r"Executed (?P<total>\d+) tests?, with (?P<failed>\d+) failures?",
    )
    .captures_iter(output)
    .last()
    {
        let total = number(&c, "total");
        summary.failed = number(&c, "failed");
        summary.passed = total.saturating_sub(summary.failed);
        return Some(summary);
    }
    (!summary.is_empty()).then_some(summary)
}

fn is_failure_line(line: &str) -> bool {
    static FAILURE: OnceLock<Regex> = OnceLock::new();
    re(
        &FAILURE,
        r"(?i)(^error(\[|:)|panicked at|^FAILED|^E\s{2,}|Traceback \(most recent|AssertionError|✗|×|● |--- FAIL|\bFAIL\b)",
    )
    .is_match(line.trim_start())
}

/// The lines the agent sees: all of short output; otherwise the first
/// failure's neighbourhood and the tail, each marked where lines were left out.
pub fn excerpt(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let clip = |line: &str| -> String {
        let mut clipped: String = line.chars().take(240).collect();
        if clipped.len() < line.len() {
            clipped.push('…');
        }
        clipped
    };
    if lines.len() <= SHOWN_LINES && output.len() <= SHOWN_BYTES {
        return lines.iter().map(|l| clip(l)).collect::<Vec<_>>().join("\n");
    }
    let tail_start = lines.len().saturating_sub(24);
    let mut picked: Vec<usize> = Vec::new();
    if let Some(first) = lines[..tail_start].iter().position(|l| is_failure_line(l)) {
        picked.extend(first.saturating_sub(2)..(first + 14).min(tail_start));
    }
    picked.extend(tail_start..lines.len());
    let mut shown = String::new();
    let mut last: Option<usize> = None;
    for index in picked {
        let gap = match last {
            None => index,
            Some(previous) => index - previous - 1,
        };
        if gap > 0 {
            shown.push_str(&format!("… {gap} lines\n"));
        }
        let line = clip(lines[index]);
        if shown.len() + line.len() > SHOWN_BYTES {
            shown.push_str(&format!("… {} lines\n", lines.len() - index));
            break;
        }
        shown.push_str(&line);
        shown.push('\n');
        last = Some(index);
    }
    shown.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_counts_and_failures() {
        let output = "running 3 tests\ntest a::one ... ok\ntest a::two ... FAILED\ntest a::three ... ignored\n\nfailures:\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out\n\nrunning 2 tests\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured\n";
        let summary = tests(output).unwrap();
        assert_eq!((summary.passed, summary.failed, summary.skipped), (3, 1, 1));
        assert_eq!(summary.failures, ["a::two"]);
        assert_eq!(summary.line(), "3 passed, 1 failed, 1 skipped (a::two)");
    }

    #[test]
    fn other_runners() {
        let pytest = "FAILED tests/test_add.py::test_adds_one - assert 2 == 3\n=========== 1 failed, 12 passed, 2 skipped in 0.42s ===========\n";
        let summary = tests(pytest).unwrap();
        assert_eq!(
            (summary.passed, summary.failed, summary.skipped),
            (12, 1, 2)
        );
        assert_eq!(summary.failures, ["tests/test_add.py::test_adds_one"]);

        let jest =
            "FAIL src/add.test.ts\n  ● add › adds one\nTests:       1 failed, 7 passed, 8 total\n";
        let summary = tests(jest).unwrap();
        assert_eq!((summary.passed, summary.failed), (7, 1));
        assert!(summary.failures.contains(&"add › adds one".to_owned()));

        let vitest =
            " Test Files  1 failed | 3 passed (4)\n      Tests  2 failed | 40 passed (42)\n";
        let summary = tests(vitest).unwrap();
        assert_eq!((summary.passed, summary.failed), (40, 2));

        let go = "=== RUN   TestAdd\n--- FAIL: TestAdd (0.00s)\n--- PASS: TestSub (0.00s)\nFAIL\n";
        let summary = tests(go).unwrap();
        assert_eq!((summary.passed, summary.failed), (1, 1));
        assert_eq!(summary.failures, ["TestAdd"]);

        let mocha = "  12 passing (40ms)\n  1 pending\n  2 failing\n";
        let summary = tests(mocha).unwrap();
        assert_eq!(
            (summary.passed, summary.skipped, summary.failed),
            (12, 1, 2)
        );

        let unittest = "test_a (test_p.T) ... ok\ntest_b (test_p.T) ... FAIL\n\n======\nFAIL: test_b (test_p.T)\n------\nRan 2 tests in 0.001s\n\nFAILED (failures=1)\n";
        let summary = tests(unittest).unwrap();
        assert_eq!((summary.passed, summary.failed), (1, 1));
        assert_eq!(summary.failures, ["test_b"]);
        let summary = tests("Ran 3 tests in 0.002s\n\nOK (skipped=1)\n").unwrap();
        assert_eq!((summary.passed, summary.failed, summary.skipped), (2, 0, 1));

        assert!(tests("Compiling zephium v0.1.0\nFinished dev").is_none());
    }

    #[test]
    fn excerpt_keeps_the_failure_and_the_tail() {
        let short = "one\ntwo";
        assert_eq!(excerpt(short), short);
        let mut long: Vec<String> = (0..300).map(|n| format!("compiling {n}")).collect();
        long[100] = "error[E0308]: mismatched types".into();
        let shown = excerpt(&long.join("\n"));
        assert!(shown.contains("error[E0308]"));
        assert!(shown.contains("compiling 299"));
        assert!(shown.contains("… 98 lines"));
        assert!(shown.len() <= SHOWN_BYTES + 64);
        assert!(!shown.contains("compiling 50\n"));
    }
}
