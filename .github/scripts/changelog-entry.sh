#!/bin/sh
# Writes one release's section into CHANGELOG.md, from the commit subjects of
# the range being released.
#
# Lives in a file rather than inline in the workflow for the reason
# raise-version.sh does: a filter decides what a user is told about a release,
# and a filter nothing drives is a wish. The test feeds it fixed subjects and
# reads back the section, so the rules below are checkable without a release.
#
# Subjects arrive on stdin, one per line, newest first, which is what
# `git log --format=%s` gives and the order the section is written in.
#
# Usage: changelog-entry.sh <version> <date> [changelog]
#   version   the version being released, three numeric components
#   date      ISO date for the heading
#   changelog the file to write into (default: ./CHANGELOG.md)
set -eu

[ "$#" -ge 2 ] || { echo "::error::usage: changelog-entry.sh <version> <date> [changelog]"; exit 1; }
version="$1"
date="$2"
changelog="${3:-CHANGELOG.md}"

case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "::error::${version} is not three numeric components"; exit 1 ;;
esac

[ -f "$changelog" ] || { echo "::error::${changelog} does not exist"; exit 1; }

# A section for this version already existing means the release is being
# re-run, or that somebody wrote it by hand. Either way, overwriting it would
# discard the better text, so this stops instead.
if grep -qF "## [${version}]" "$changelog"; then
    echo "::error::${changelog} already carries a section for ${version}"
    exit 1
fi

# **The two filters are different rules and neither subsumes the other.**
#
# The first drops the repository's own prefix convention: a subject beginning
# `roadmap:` or `spec:` is work on the written layer, which a user of the binary
# cannot observe. `feat:` and `fix:` are deliberately absent, because those
# describe the product and predate the prose-title convention.
#
# The second drops the subjects that describe the same work without a prefix,
# which is most of them: a row moving on the roadmap, a phase reordering, a
# ruling being written, a token the release needed. It matches anywhere in the
# subject rather than at an anchor, because the internal thing is as often the
# object as the subject, and case-insensitively, because `ROADMAP.md` and `the
# roadmap` are the same subject twice.
#
# **Scoping the range to `crates/` instead was tried and does not work here.**
# It is the usual way to do this and it looks stricter, but this repository
# gates its own prose from tests, so a commit that only edits ROADMAP.md still
# touches `crates/vigia/tests/package.rs` to move a ceiling. Measured over
# 0.34.0 to 0.38.0, the path scope kept every document commit in the range.
#
# The list is tuned against the subjects this repository has and is a heuristic,
# not a rule, and its two directions do not cost the same. Something internal
# that slips through is one odd line a reader skims past. A visible change it
# drops is a reader upgrading into a pane with a key missing and notes saying
# nothing moved, and nothing downstream can catch that, because the section is
# the only place the change was ever going to be named. That asymmetry is why
# the allow pass below wins over this list, and why an emptied range is reported
# rather than summarised.
#
# `token` is bounded the long way round rather than with `\b`, which is a GNU
# extension that POSIX ERE does not define. The release runs this on one runner
# whose grep has it, but the test that drives the filter is `cfg(unix)` and so
# runs wherever the suite does. Unbounded, the word would swallow `tokenizer`,
# which in a syntax highlighter is a subject a reader can see.
internal_prefix='^(roadmap|spec|docs?|ci|chore|deps?|take-next|skill|test|refactor|perf|style|build|process|steering|mockup|harden|release|vault writes)(\([^)]*\))?: '
internal_subject='(roadmap|spec\.md|the spec|rulings?|revocation|withdrawn|written layer|phase [0-9]|the shelf|shelved|readme|claude\.md|clippy|cargo doc|ci complete|workflow|pre-flight|version raise|release note|the release |the bump |(^|[^a-zA-Z])tokens?([^a-zA-Z]|$)|take-next|the skill|the harness|the record|budget table|mutation|audit|ceiling|proposal|declined|adopted|review agent|assertion|the mockup|funding|\.yml|§|^track the |^#[0-9]|^b[0-9]+[ :]|^[0-9]+\.[0-9]+,)'

# **A subject naming something a reader can press or set survives that list.**
# The list matches anywhere in a subject, so one internal word decides a whole
# sentence, and the rule that a revoked ruling is deleted alongside the change it
# governed makes that sentence ordinary: the subject removing a key names the
# ruling too. The backticks are what make `single`, `wrap` and `links` safe to
# name.
#
# The character class cannot go stale and the settings are held to `config.rs` by
# a test. The named keys are typed here, and a new one reaches this file by hand.
visible_subject='`([A-Za-z?/]|Esc|Enter|Tab|Space|Home|End|PgUp|PgDn|Page (Up|Down)|Up|Down|Left|Right|rail|single|overview|staged|wrap|notes|icons|links|hide)`'

# Read once, because the emptied-range branch below writes the range out and
# cannot go back to stdin for it.
subjects=$(cat)

# `|| true` on the grep, because it exits 1 when it filters everything out and
# `set -e` would kill the script on the assignment rather than let the empty
# case below be handled.
#
# The second pass is `awk` rather than a second `grep` because its rule is a
# conjunction, dropping a subject the list matches and the allow pass does not,
# and splitting that over two greps would reorder the range, which is written
# newest first. The patterns reach it through the environment rather than `-v`,
# for the reason the entries do below: `-v` interprets backslash escapes, and
# both patterns carry `\.`.
kept=$(printf '%s\n' "$subjects" | grep -Ev "$internal_prefix" || true)
kept=$(printf '%s\n' "$kept" | INTERNAL="$internal_subject" VISIBLE="$visible_subject" awk '
    $0 ~ ENVIRON["VISIBLE"] { print; next }
    tolower($0) ~ ENVIRON["INTERNAL"] { next }
    { print }
')

# **Every subject the filter drops is named in the run's log.** A drop is
# invisible everywhere else: the section is the only place the change was going
# to appear, so a range that keeps nine subjects and loses the tenth reads as a
# complete section, and the emptied-range branch below cannot see that case.
#
# Matched whole rather than by pattern, because a subject carrying `?` or `[` is
# a subject about a key and `case` would read those as globs.
printf '%s\n' "$subjects" | KEPT="$kept" awk '
    BEGIN { n = split(ENVIRON["KEPT"], k, "\n"); for (i = 1; i <= n; i++) keep[k[i]] = 1 }
    NF && !($0 in keep) { print "::notice::filtered as internal: " $0 }
'

# Trailing `(#123)` references are the tracker's, not the reader's. The loop
# peels them one at a time because a merge subject carries the issue's number
# and the pull request's, and sometimes two issues'.
strip_references() {
    sed -E -e :a \
           -e 's/[[:space:]]*\((#[0-9]+(,[[:space:]]*#[0-9]+)*)\)[[:space:]]*$//' \
           -e ta \
           -e 's/[[:space:]]+$//' \
        | grep -v '^$' \
        || true
}

entries=$(printf '%s\n' "$kept" | strip_references | sed -E 's/^/- /')

# **An empty section is written rather than skipped, and an emptied range says
# what it held.** A version with no section makes `dist` fall back to the
# install instructions alone, with no warning anywhere, so the release that had
# nothing to report and the release whose notes were lost look identical to a
# reader. A sentence claiming nothing moved does not tell them apart either: it
# is a conclusion drawn from a word list's silence over free prose, and twice in
# this repository's releases that list emptied a range because a subject named a
# key and a ruling in one breath. So the range is written out instead, and the
# reader has the thing the filter could not see.
if [ -z "$entries" ]; then
    range=$(printf '%s\n' "$subjects" | strip_references | sed -E 's/^/  - /')
    if [ -z "$range" ]; then
        entries="- No commit sits between this release and the one before it."
    else
        entries=$(printf '%s\n%s\n' \
            "- Nothing in this release's commits matched what a reader of the pane can see. That is as likely to be this filter dropping a change as a release with nothing in it, so every commit in the range is listed here unfiltered rather than summarised:" \
            "$range")
    fi
fi

# Written above the newest existing section, so the file stays newest first.
# `awk` rather than `sed`, because the insertion is a block and the anchor is
# the first line of many that match.
#
# The entries reach awk through a file rather than through `-v`, which
# interprets backslash escapes in the value it is given: a commit subject
# carrying one would arrive mangled, and a subject ending in one would eat the
# line after it.
lines="${changelog}.entries"
printf '%s\n' "$entries" > "$lines"

inserted="${changelog}.inserted"
awk -v heading="## [${version}] - ${date}" -v lines="$lines" '
    function emit() {
        print heading
        print ""
        while ((getline entry < lines) > 0) print entry
        close(lines)
    }
    !written && /^## \[/ {
        emit()
        print ""
        written = 1
    }
    { print }
    END { if (!written) emit() }
' "$changelog" > "$inserted"
mv "$inserted" "$changelog"
rm -f "$lines"

# Verify the write rather than trusting it, on raise-version.sh's rule: an awk
# that matched nothing exits 0 and leaves the file as it was.
if ! grep -qF "## [${version}] - ${date}" "$changelog"; then
    echo "::error::the section for ${version} was not written to ${changelog}"
    exit 1
fi

echo "::notice::wrote $(printf '%s\n' "$entries" | wc -l | tr -d ' ') entry line(s) for ${version}"
