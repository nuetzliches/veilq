# ADR-0044: A deny list names the people a text can name

- **Status:** Accepted (card in issue #49, points B1 to B6, approved by the owner 2026-11-13); amends [ADR-0004](0004-the-policy-is-veilq-toml-in-the-current-directory-or-the-git-root.md) (a new section) and [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md)
- **Date:** 2026-11-13
- **Deciders:** project owner
- **Related:** [ADR-0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md), [ADR-0040](0040-purposes-are-built-in-policy-packs.md), [ADR-0042](0042-a-column-of-a-list-names-what-its-cells-hold.md), #34

## Context

When the people a text can mention are known beforehand (a member list,
the participants of a case), a list of their names is the most reliable
detector there is. Until now it took a hand-written regex in
`[[patterns]]`, which misses the genitive (`Mehmets Auto`), the case of
informal text (`ayşe`, `MEHMET`), Turkish `İ`/`ı`, and keeps the names
inside the policy.

## Decision

1. **`[[deny]]` tables** in the policy, each with `entity` (`PERSON` by
   default), `values` and `files`, and `parts` (default `true`). A table
   with neither values nor files is an error.
2. **`files` are relative to the policy file** that names them: UTF-8, one
   value per line, blank lines and lines starting with `#` skipped. They
   are read when the policy is loaded; a file that cannot be read is an
   error (fail closed), and so is a table whose files were never read (a
   policy parsed from text without its file). Written back (the UI's user
   policy), the policy names its files, not their values.
3. **Matching:** each value as a whole word, case-insensitively, in NFC and
   NFD, with Turkish `İ`, `I`, `ı` and `i` as one letter and the space
   between words as any white space, with the genitive `s`, `'s` or `’s`;
   at confidence 1.0, detector `policy:deny`, in every format and in text
   read by OCR. The values are one regex per table, the longest first.
4. **Parts:** with `parts`, a value of several words also finds each word
   of three letters or more alone, except role words (`NOT_NAMES`):
   `Ayşe Yılmaz` finds `Yılmaz` in `Herr Yilmaz`.
5. **Precedence:** a deny value is found where `allow` names it, is never
   dropped as a role word, an account or a brand, and takes its span from
   a candidate below its threshold or one the allow lists name. Purposes
   (ADR-0040) do not change the lists.
6. **Values stay out of sight:** the report holds no values anyway
   ([ADR-0002](0002-a-finding-never-carries-the-value.md)); `--explain` shows the match as it shows every match. The UI
   shows a deny list by its size (values and files), not its names, and
   keeps the tables as they are when it saves the user's policy. The
   README advises keeping name files out of the repository.

Point 5 of the card said the lists of the project's and the user's policy
add up, as `allow` does. Only one policy file is in effect (ADR-0028: the
project's, else the user's), and `allow` does not add up either; the lists
of the policy in effect apply.

## Alternatives considered

- **One `[deny]` table:** one type only; names and member numbers would
  need two policies.
- **Values inside the policy only:** a list changes more often than a
  policy, and a policy is often in a repository a list must not be in.
- **A missing file skipped:** the list would silently find nothing.
- **Folding the text before matching:** the offsets of a finding would no
  longer be the text's; a character class per `i`-letter keeps them.

## Consequences

- A policy can carry the most reliable name detector without regex.
- A list of names is personal data: the policy file that names it stays
  shareable, the list beside it does not.
- A very long list makes one large regex (a size limit of 64 MB per
  table); a list beyond it is an error, not a silent cut.
- The first step of #34 ("the exact value becomes a deny entry") has its
  mechanism.

## Enforced by

- `crates/veilq-core/src/policy.rs` (`Deny`, `Policy::read_deny_files`,
  `validate`), test `deny_files_are_read_relative_to_their_policy`
- `crates/veilq-core/src/detect/mod.rs` (`deny_rule`, `DENY`, `decide`),
  tests `a_deny_list_finds_its_values_in_every_form`,
  `a_deny_list_whose_files_were_not_read_is_an_error`
- `crates/veilq/src/ui_settings.rs` (files relative to the user's policy),
  `ui/src/components/RulesCard.vue` (the size only)
- `schema/policy.schema.json`
