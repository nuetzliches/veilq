# ADR-0028: A user policy, edited in the UI, and folders redacted whole

- **Status:** Accepted (card of ADR-0027, points 2 and 3, approved by the owner 2026-11-11); amends [ADR-0004](0004-the-policy-is-veilq-toml-in-the-current-directory-or-the-git-root.md) (where the policy is found) and [ADR-0008](0008-the-web-ui-is-local-token-bound-and-reads-under-one-root.md) (the root can be chosen, the UI writes into a target folder)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md)

## Context

Someone who starts veilq by double click has no project and no
`veilq.toml`, and no editor to write one: the firm's own name is found as a
person, a customer number is not found at all. The UI read files under the
folder it was started in, which for a double click is the home folder, and
redacted one file at a time.

## Decision

- **The user's policy:** `%APPDATA%\veilq\veilq.toml` on Windows,
  `~/Library/Application Support/veilq/veilq.toml` on macOS,
  `$XDG_CONFIG_HOME/veilq/veilq.toml` (or `~/.config/...`) elsewhere;
  `VEILQ_USER_POLICY` names another file. It is found after `veilq.toml`
  in the current folder and in the git root, before the built-in defaults:
  a project's policy comes first, for the CLI, the MCP server and the UI
  alike.
- **Rules in Settings** edit that file as a form: the default action and
  confidence, the languages, per type an action and a threshold, values
  and patterns that are never personal data, and the user's own patterns.
  "Try" checks the policy as its file and runs its patterns (no models) on
  a sample; "Save" checks it, builds the engine with it where it is the
  policy in use, and only then writes it. The page says when a project's
  `veilq.toml` is in use instead.
- **The folder:** Settings lists folders of the machine (names only, from
  home, Documents, Desktop, Downloads and the drives) and makes one the
  root; a double-click veilq keeps it in `%LOCALAPPDATA%\veilq\settings.json`.
- **A folder run** (the Folder tab): every file under a folder of the root
  is redacted into a new or empty folder outside it, written only when its
  redaction succeeded; a file veilq cannot read is listed with its reason
  and not copied. The target gets `veilq-report.json`: per file its types
  and counts, or its error, never a value. Pictures that do not settle are
  errors unless the run asks for `unverified` (ADR-0026).

## Alternatives considered

- **Editing the TOML as text in the UI:** no help for someone who has never
  seen one; the form shows the file it writes, for those who want it.
- **The user policy before the project's:** a project's rules would be
  overridden by whoever runs veilq there.
- **A folder run that copies what it cannot read:** fail closed (AGENTS.md,
  point 7) forbids it.

## Consequences

- A user policy now changes CLI and MCP runs outside a project too; `veilq
  policy check` names the file in use.
- The UI writes the user's policy and into a target folder the user chose,
  behind the token. Choosing the root lets the File tab read under any
  folder the user picks.
- The form covers what most need; mapping recipients, the pseudonym key's
  variable, Office and marker settings stay as they are in the file and
  are written back unchanged.

## Enforced by

- `crates/veilq-core/src/policy.rs` (`user_file`, `discover`, `to_toml`)
- `crates/veilq/src/ui_settings.rs` (`/api/policy`, `/api/policy/try`,
  `/api/folders`, `/api/root`, `/api/batch`)
- `ui/src/components/RulesCard.vue`, `FolderPicker.vue`, `FolderRun.vue`
- Tests: `policy::tests::the_user_policy_comes_after_the_project`,
  `policy::tests::a_policy_written_as_toml_reads_the_same`,
  `ui::tests::a_policy_is_tried_on_a_sample_and_a_bad_one_refused`,
  `ui::tests::a_folder_run_writes_beside_and_lists_what_it_cannot_read`,
  `ui::tests::folders_are_listed_by_name_only`
