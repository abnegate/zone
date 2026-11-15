use super::*;
use abnegate_vcs::FileStatus;
use serde_json::json;

const REPOSITORY_URL: &str = "https://github.com/acme/widgets";
const HEAD: &str = "1111111111111111111111111111111111111111";
const BASE: &str = "2222222222222222222222222222222222222222";
const NUMBER: u64 = 42;

fn revision() -> Revision<'static> {
    Revision {
        repository_url: REPOSITORY_URL,
        number: NUMBER,
        head: HEAD,
        base: BASE,
    }
}

fn parsed(text: &str) -> Diff {
    parse(text, false, &revision())
}

fn only(diff: &Diff) -> &File {
    assert_eq!(diff.files.len(), 1, "expected one file: {diff:#?}");
    &diff.files[0]
}

fn line(kind: LineKind, content: &str, old_line: Option<u32>, new_line: Option<u32>) -> Line {
    Line {
        kind,
        content: content.to_string(),
        old_line,
        new_line,
    }
}

fn blob(commit: &str, path: &str) -> String {
    format!("{REPOSITORY_URL}/blob/{commit}/{path}")
}

const MODIFIED: &str = "\
diff --git a/src/cart.rs b/src/cart.rs
index 3b18e51..a9c4f12 100644
--- a/src/cart.rs
+++ b/src/cart.rs
@@ -10,4 +10,5 @@ impl Cart {
     fn total(&self) -> u32 {
-        self.items.len()
+        let count = self.items.len();
+        count
     }
 }
";

#[test]
fn a_modified_file_reads_as_one_hunk_with_numbered_lines() {
    let diff = parsed(MODIFIED);

    assert_eq!(diff.number, NUMBER);
    assert_eq!(diff.head, HEAD);
    assert_eq!(diff.base, BASE);
    assert!(diff.complete);
    assert_eq!(diff.warning, None);
    let file = only(&diff);
    assert_eq!(file.path, "src/cart.rs");
    assert_eq!(file.previous_path, None);
    assert_eq!(file.status, Status::Modified);
    assert_eq!((file.additions, file.deletions, file.changes), (2, 1, 3));
    assert!(!file.binary);
    assert!(!file.truncated);
    assert_eq!(file.blob_url, blob(HEAD, "src/cart.rs"));
    assert_eq!(
        file.hunks,
        vec![Hunk {
            header: "@@ -10,4 +10,5 @@ impl Cart {".to_string(),
            old_start: 10,
            old_lines: 4,
            new_start: 10,
            new_lines: 5,
            lines: vec![
                line(
                    LineKind::Context,
                    "    fn total(&self) -> u32 {",
                    Some(10),
                    Some(10)
                ),
                line(
                    LineKind::Deletion,
                    "        self.items.len()",
                    Some(11),
                    None
                ),
                line(
                    LineKind::Addition,
                    "        let count = self.items.len();",
                    None,
                    Some(11)
                ),
                line(LineKind::Addition, "        count", None, Some(12)),
                line(LineKind::Context, "    }", Some(12), Some(13)),
                line(LineKind::Context, "}", Some(13), Some(14)),
            ],
        }]
    );
}

#[test]
fn several_files_and_hunks_each_keep_their_own_numbering() {
    let text = "\
diff --git a/one.txt b/one.txt
--- a/one.txt
+++ b/one.txt
@@ -1,2 +1,2 @@
-first
+First
 second
@@ -20 +20 @@ fn tail()
-old tail
+new tail
diff --git a/two.txt b/two.txt
--- a/two.txt
+++ b/two.txt
@@ -5,0 +6,1 @@
+inserted
";

    let diff = parsed(text);

    assert!(diff.complete);
    assert_eq!(diff.files.len(), 2);
    let one = &diff.files[0];
    assert_eq!(one.hunks.len(), 2);
    assert_eq!(
        (one.hunks[1].old_start, one.hunks[1].old_lines),
        (20, 1),
        "a range without a count covers one line"
    );
    assert_eq!(
        one.hunks[1].lines,
        vec![
            line(LineKind::Deletion, "old tail", Some(20), None),
            line(LineKind::Addition, "new tail", None, Some(20)),
        ]
    );
    let two = &diff.files[1];
    assert_eq!(two.path, "two.txt");
    assert_eq!(
        two.hunks[0].lines,
        vec![line(LineKind::Addition, "inserted", None, Some(6))]
    );
    assert_eq!((two.additions, two.deletions), (1, 0));
}

#[test]
fn a_new_file_is_added_and_links_to_the_head() {
    let text = "\
diff --git a/docs/new.md b/docs/new.md
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/docs/new.md
@@ -0,0 +1,2 @@
+# New
+page
";

    let file = only(&parsed(text)).clone();

    assert_eq!(file.status, Status::Added);
    assert_eq!(file.path, "docs/new.md");
    assert_eq!(file.previous_path, None);
    assert_eq!(file.blob_url, blob(HEAD, "docs/new.md"));
    assert_eq!(file.hunks[0].old_start, 0);
    assert_eq!(file.hunks[0].lines[0].new_line, Some(1));
    assert!(!file.truncated);
}

#[test]
fn a_deleted_file_is_removed_and_links_to_the_base() {
    let text = "\
diff --git a/legacy.rs b/legacy.rs
deleted file mode 100644
index 9daeafb..0000000
--- a/legacy.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn legacy() {}
-
";

    let file = only(&parsed(text)).clone();

    assert_eq!(file.status, Status::Removed);
    assert_eq!(file.path, "legacy.rs");
    assert_eq!(file.blob_url, blob(BASE, "legacy.rs"));
    assert_eq!(file.deletions, 2);
    assert_eq!(
        file.hunks[0].lines[1],
        line(LineKind::Deletion, "", Some(2), None)
    );
}

#[test]
fn a_renamed_file_keeps_its_previous_path() {
    let text = "\
diff --git a/src/basket.rs b/src/cart.rs
similarity index 88%
rename from src/basket.rs
rename to src/cart.rs
index 1234567..89abcde 100644
--- a/src/basket.rs
+++ b/src/cart.rs
@@ -1 +1 @@
-struct Basket;
+struct Cart;
diff --git a/old name.txt b/new name.txt
similarity index 100%
rename from old name.txt
rename to new name.txt
";

    let diff = parsed(text);

    assert!(diff.complete);
    let edited = &diff.files[0];
    assert_eq!(edited.status, Status::Renamed);
    assert_eq!(edited.path, "src/cart.rs");
    assert_eq!(edited.previous_path.as_deref(), Some("src/basket.rs"));
    assert_eq!(edited.blob_url, blob(HEAD, "src/cart.rs"));
    let moved = &diff.files[1];
    assert_eq!(moved.status, Status::Renamed);
    assert_eq!(moved.path, "new name.txt");
    assert_eq!(moved.previous_path.as_deref(), Some("old name.txt"));
    assert!(moved.hunks.is_empty());
    assert!(!moved.binary);
    assert!(!moved.truncated);
    assert_eq!(moved.blob_url, blob(HEAD, "new%20name.txt"));
}

#[test]
fn a_copied_file_keeps_its_source_as_the_previous_path() {
    let text = "\
diff --git a/template.toml b/config.toml
similarity index 100%
copy from template.toml
copy to config.toml
";

    let file = only(&parsed(text)).clone();

    assert_eq!(file.status, Status::Copied);
    assert_eq!(file.path, "config.toml");
    assert_eq!(file.previous_path.as_deref(), Some("template.toml"));
}

#[test]
fn binary_files_have_no_hunks_and_are_not_truncated() {
    let text = "\
diff --git a/logo.png b/logo.png
index 1111111..2222222 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/icon.png b/icon.png
new file mode 100644
index 0000000..3333333
GIT binary patch
literal 12
TcmZ?wbhEHbRA2yt0Kx!s0RR91

literal 0
HcmV?d00001

diff --git a/after.txt b/after.txt
--- a/after.txt
+++ b/after.txt
@@ -1 +1 @@
-a
+b
";

    let diff = parsed(text);

    assert!(diff.complete, "{diff:#?}");
    assert_eq!(diff.files.len(), 3);
    for binary in &diff.files[..2] {
        assert!(binary.binary, "{} is binary", binary.path);
        assert!(binary.hunks.is_empty());
        assert!(!binary.truncated);
        assert_eq!(binary.changes, 0);
    }
    assert_eq!(diff.files[1].status, Status::Added);
    assert!(!diff.files[2].binary);
    assert_eq!(diff.files[2].changes, 2);
}

#[test]
fn a_missing_final_newline_is_a_meta_line_without_numbers() {
    let text = "\
diff --git a/version b/version
--- a/version
+++ b/version
@@ -1 +1 @@
-1.0.0
\\ No newline at end of file
+1.0.1
\\ No newline at end of file
";

    let diff = parsed(text);

    assert!(diff.complete, "{diff:#?}");
    let file = only(&diff);
    assert_eq!((file.additions, file.deletions), (1, 1));
    assert_eq!(
        file.hunks[0].lines,
        vec![
            line(LineKind::Deletion, "1.0.0", Some(1), None),
            line(LineKind::Meta, "\\ No newline at end of file", None, None),
            line(LineKind::Addition, "1.0.1", None, Some(1)),
            line(LineKind::Meta, "\\ No newline at end of file", None, None),
        ]
    );
}

#[test]
fn quoted_paths_are_unescaped_and_linked_encoded() {
    let text = "\
diff --git \"a/caf\\303\\251 menu.txt\" \"b/caf\\303\\251 menu.txt\"
--- \"a/caf\\303\\251 menu.txt\"
+++ \"b/caf\\303\\251 menu.txt\"
@@ -1 +1 @@
-old
+new
diff --git \"a/tab\\there\" \"b/tab\\there\"
deleted file mode 100644
";

    let diff = parsed(text);

    assert_eq!(diff.files[0].path, "café menu.txt");
    assert_eq!(diff.files[0].blob_url, blob(HEAD, "caf%C3%A9%20menu.txt"));
    assert_eq!(diff.files[1].path, "tab\there");
    assert_eq!(diff.files[1].status, Status::Removed);
    assert_eq!(diff.files[1].blob_url, blob(BASE, "tab%09here"));
}

#[test]
fn unquoted_paths_with_spaces_drop_the_trailing_tab() {
    let text = "\
diff --git a/my notes.txt b/my notes.txt
--- a/my notes.txt\t
+++ b/my notes.txt\t
@@ -1 +1 @@
-x
+y
";

    let file = only(&parsed(text)).clone();

    assert_eq!(file.path, "my notes.txt");
    assert_eq!(file.blob_url, blob(HEAD, "my%20notes.txt"));
}

#[test]
fn a_header_alone_names_a_file_with_spaces() {
    let text = "\
diff --git a/a b/c.txt b/a b/c.txt
old mode 100644
new mode 100755
";

    let file = only(&parsed(text)).clone();

    assert_eq!(file.path, "a b/c.txt");
    assert_eq!(file.status, Status::Modified);
    assert!(file.hunks.is_empty());
    assert!(!file.truncated);
}

#[test]
fn hunk_lines_that_look_like_headers_stay_in_the_hunk() {
    let text = "\
diff --git a/fixture.diff b/fixture.diff
--- a/fixture.diff
+++ b/fixture.diff
@@ -1,3 +1,3 @@
 diff --git a/x b/x
--- a/x
+++ b/x
-@@ -1 +1 @@
+@@ -2 +2 @@
";

    let diff = parsed(text);

    assert!(diff.complete, "{diff:#?}");
    let file = only(&diff);
    assert_eq!(file.path, "fixture.diff");
    assert_eq!(
        file.hunks[0].lines,
        vec![
            line(LineKind::Context, "diff --git a/x b/x", Some(1), Some(1)),
            line(LineKind::Deletion, "-- a/x", Some(2), None),
            line(LineKind::Addition, "++ b/x", None, Some(2)),
            line(LineKind::Deletion, "@@ -1 +1 @@", Some(3), None),
            line(LineKind::Addition, "@@ -2 +2 @@", None, Some(3)),
        ]
    );
}

#[test]
fn a_blank_line_inside_a_hunk_is_empty_context() {
    let text = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 one

-three
+drei
";

    let diff = parsed(text);

    assert!(diff.complete, "{diff:#?}");
    assert_eq!(
        only(&diff).hunks[0].lines[1],
        line(LineKind::Context, "", Some(2), Some(2))
    );
}

#[test]
fn carriage_returns_stay_part_of_the_content() {
    let text = "diff --git a/w.bat b/w.bat\n--- a/w.bat\n+++ b/w.bat\n@@ -1 +1 @@\n-echo off\r\n+echo on\r\n";

    let file = only(&parsed(text)).clone();

    assert_eq!(file.hunks[0].lines[1].content, "echo on\r");
}

#[test]
fn a_preamble_before_the_first_file_is_ignored() {
    let text = format!("From 1234 Mon Sep 17 00:00:00 2001\nSubject: change\n---\n{MODIFIED}");

    let diff = parsed(&text);

    assert!(diff.complete);
    assert_eq!(only(&diff).path, "src/cart.rs");
}

#[test]
fn a_truncated_excerpt_marks_only_the_last_file_and_drops_its_partial_line() {
    let text = "\
diff --git a/first.txt b/first.txt
--- a/first.txt
+++ b/first.txt
@@ -1 +1 @@
-a
+b
diff --git a/second.txt b/second.txt
--- a/second.txt
+++ b/second.txt
@@ -1,3 +1,3 @@
 keep
-gone
+half a li";

    let diff = parse(text, true, &revision());

    assert!(!diff.complete);
    assert_eq!(diff.warning.as_deref(), Some(TRUNCATED_DIFF));
    assert_eq!(diff.files.len(), 2);
    assert!(!diff.files[0].truncated);
    let last = &diff.files[1];
    assert!(last.truncated);
    assert_eq!(
        last.hunks[0].lines,
        vec![
            line(LineKind::Context, "keep", Some(1), Some(1)),
            line(LineKind::Deletion, "gone", Some(2), None),
        ],
        "the cut line is dropped rather than shown as content"
    );
    assert_eq!((last.additions, last.deletions), (0, 1));
}

#[test]
fn a_truncated_excerpt_cut_at_a_file_boundary_still_marks_the_last_file() {
    let diff = parse(MODIFIED, true, &revision());

    assert!(!diff.complete);
    assert!(only(&diff).truncated);
    assert_eq!(only(&diff).hunks[0].lines.len(), 6);
}

#[test]
fn an_unreadable_hunk_header_marks_the_file_and_the_diff_incomplete() {
    let text = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -x +1 @@
+y
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-p
+q
";

    let diff = parsed(text);

    assert!(!diff.complete);
    assert_eq!(diff.warning.as_deref(), Some(UNREADABLE_PATCH));
    assert!(diff.files[0].truncated);
    assert!(diff.files[0].hunks.is_empty());
    assert!(!diff.files[1].truncated);
}

#[test]
fn an_empty_diff_is_complete_and_has_no_files() {
    let diff = parsed("");

    assert!(diff.complete);
    assert!(diff.files.is_empty());
    assert_eq!(diff.warning, None);
}

#[test]
fn a_patch_reads_hunks_and_counts() {
    let patch = patch("@@ -1,2 +1,2 @@\n-a\n+b\n c\n\\ No newline at end of file");

    assert!(!patch.truncated);
    assert_eq!((patch.additions, patch.deletions), (1, 1));
    assert_eq!(patch.hunks.len(), 1);
    assert_eq!(patch.hunks[0].lines.len(), 4);
    assert_eq!(patch.hunks[0].lines[3].kind, LineKind::Meta);
}

#[test]
fn an_empty_patch_has_no_hunks_and_is_not_truncated() {
    assert_eq!(
        patch(""),
        Patch {
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
            truncated: false,
        }
    );
}

#[test]
fn a_patch_shorter_than_its_header_is_truncated() {
    let patch = patch("@@ -1,3 +1,3 @@\n a\n-b");

    assert!(patch.truncated);
    assert_eq!(patch.deletions, 1);
}

#[test]
fn a_patch_with_lines_before_any_hunk_is_truncated() {
    assert!(patch("stray\n@@ -1 +1 @@\n-a\n+b").truncated);
}

#[test]
fn per_file_patches_build_an_incomplete_diff_with_a_warning() {
    let entries = [
        Entry {
            path: "src/lib.rs",
            previous_path: None,
            status: Status::Modified,
            additions: 1,
            deletions: 1,
            patch: Some("@@ -3 +3 @@\n-old\n+new"),
        },
        Entry {
            path: "assets/logo.png",
            previous_path: None,
            status: Status::Modified,
            additions: 0,
            deletions: 0,
            patch: None,
        },
        Entry {
            path: "data/huge.json",
            previous_path: None,
            status: Status::Modified,
            additions: 90_000,
            deletions: 10,
            patch: None,
        },
        Entry {
            path: "src/cart.rs",
            previous_path: Some("src/basket.rs"),
            status: Status::Renamed,
            additions: 0,
            deletions: 0,
            patch: None,
        },
        Entry {
            path: "src/short.rs",
            previous_path: None,
            status: Status::Modified,
            additions: 5,
            deletions: 0,
            patch: Some("@@ -1 +1,2 @@\n a\n+b"),
        },
        Entry {
            path: "gone.txt",
            previous_path: None,
            status: Status::Removed,
            additions: 0,
            deletions: 1,
            patch: Some("@@ -1 +0,0 @@\n-bye"),
        },
    ];

    let diff = per_file(entries, &revision());

    assert!(!diff.complete);
    assert_eq!(diff.warning.as_deref(), Some(PER_FILE_DIFF));
    assert_eq!(diff.number, NUMBER);
    let [edited, image, huge, moved, short, gone] = diff.files.as_slice() else {
        panic!("expected six files: {diff:#?}");
    };
    assert!(!edited.truncated && !edited.binary);
    assert_eq!(edited.hunks[0].lines[0].old_line, Some(3));
    assert_eq!(edited.changes, 2);
    assert!(image.binary && !image.truncated);
    assert!(huge.truncated && !huge.binary);
    assert_eq!(huge.changes, 90_010);
    assert!(!moved.binary && !moved.truncated);
    assert_eq!(moved.previous_path.as_deref(), Some("src/basket.rs"));
    assert!(
        short.truncated,
        "a patch counting fewer lines than GitHub lists is truncated"
    );
    assert_eq!(short.additions, 5, "GitHub's own counts are kept");
    assert!(!gone.truncated);
    assert_eq!(gone.blob_url, blob(BASE, "gone.txt"));
}

#[test]
fn a_removed_file_with_a_previous_path_links_to_that_path_at_the_base() {
    let entry = Entry {
        path: "now.txt",
        previous_path: Some("before.txt"),
        status: Status::Removed,
        additions: 0,
        deletions: 0,
        patch: None,
    };

    let diff = per_file([entry], &revision());

    assert_eq!(diff.files[0].blob_url, blob(BASE, "before.txt"));
}

#[test]
fn a_trailing_slash_on_the_repository_url_is_not_doubled() {
    let revision = Revision {
        repository_url: "https://github.com/acme/widgets/",
        ..revision()
    };

    let diff = parse(MODIFIED, false, &revision);

    assert_eq!(only(&diff).blob_url, blob(HEAD, "src/cart.rs"));
}

#[test]
fn every_github_file_status_maps_to_its_own_status() {
    for (github, status) in [
        (FileStatus::Added, Status::Added),
        (FileStatus::Removed, Status::Removed),
        (FileStatus::Modified, Status::Modified),
        (FileStatus::Renamed, Status::Renamed),
        (FileStatus::Copied, Status::Copied),
        (FileStatus::Changed, Status::Changed),
        (FileStatus::Unchanged, Status::Unchanged),
        (FileStatus::Unknown, Status::Unknown),
    ] {
        assert_eq!(Status::from(github), status);
    }
}

#[test]
fn a_diff_serializes_to_the_console_contract() {
    let diff = parse(
        "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-x\n\\ No newline at end of file\n+y\n",
        false,
        &revision(),
    );

    assert_eq!(
        serde_json::to_value(&diff).unwrap(),
        json!({
            "number": NUMBER,
            "head": HEAD,
            "base": BASE,
            "complete": true,
            "warning": null,
            "files": [{
                "path": "a",
                "previous_path": null,
                "status": "modified",
                "additions": 1,
                "deletions": 1,
                "changes": 2,
                "binary": false,
                "truncated": false,
                "blob_url": blob(HEAD, "a"),
                "hunks": [{
                    "header": "@@ -1 +1 @@",
                    "old_start": 1,
                    "old_lines": 1,
                    "new_start": 1,
                    "new_lines": 1,
                    "lines": [
                        {"kind": "deletion", "content": "x", "old_line": 1, "new_line": null},
                        {"kind": "meta", "content": "\\ No newline at end of file", "old_line": null, "new_line": null},
                        {"kind": "addition", "content": "y", "old_line": null, "new_line": 1},
                    ],
                }],
            }],
        })
    );
    for (status, spelled) in [
        (Status::Added, "added"),
        (Status::Removed, "removed"),
        (Status::Renamed, "renamed"),
        (Status::Copied, "copied"),
        (Status::Changed, "changed"),
        (Status::Unchanged, "unchanged"),
        (Status::Unknown, "unknown"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), json!(spelled));
    }
    assert_eq!(
        serde_json::to_value(LineKind::Context).unwrap(),
        json!("context")
    );
}
