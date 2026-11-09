# Architecture Decision Records

A change that touches an architectural point starts with a **review card** in
its pull request, which the project owner approves point by point; the ADR
follows the code and records the decision as built, with the alternatives it
beat.

## The review card

Three to seven points, each one sentence, each tagged with a category, with the
alternative it beats and what reversing it would cost. A point belongs on the
card when it is expensive to reverse **and**

- **trust:** moves a trust boundary (what leaves the machine, what a download
  may do, what an MCP client may read); the point says what an attacker gains
  if it is wrong;
- **concept:** fixes what a core term *is* (a finding, a segment, a policy);
- **contract:** fixes an outside contract (the JSON schema, the policy file,
  the placeholder format, CLI flags and output that scripts read, exit codes);
- **term:** adds a user-visible level, mode, switch or action.

A last line lists what the agent decided on its own.

## Status values

| Status | Meaning |
|---|---|
| `Proposed` | Written, not yet agreed. |
| `Accepted` | Its card points were approved by the owner. Code builds on it. |
| `Recorded` | Written down after the fact; a fact, not a verdict. |
| `Open` | A recorded question with the options laid out. Nothing builds on either side. |
| `Superseded by ADR-NNNN` | Reversed. The file stays. |

## Numbering and format

Four digits, monotonic, never reused. Copy [`0000-template.md`](0000-template.md),
keep it to about a page.

## Index

| ADR | Title | Status |
|---|---|---|
