# ADR-0035: A release is signed in its own environment, once switched on

- **Status:** Accepted (card in the conversation, points 1 to 6, approved by the owner 2026-11-11); the Windows signing step follows the choice of a provider
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [#10](https://github.com/nuetzliches/veilq/issues/10), [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md)

## Context

Users without a command line start veilq by a double click (ADR-0027).
Unsigned, Windows SmartScreen warns about an unknown publisher and macOS
Gatekeeper blocks the binary; a security scanner also takes longer over an
unknown file. A signature needs a certificate and keys the project must
keep out of everything a pull request can run. Which Windows provider to
use (Azure Artifact Signing, SignPath Foundation, an OV certificate in a
cloud HSM) is still the owner's decision; macOS has one way, a Developer ID.

## Decision

1. **Keys only in the release environment.** Signing runs in jobs with
   `environment: release` on a tag; the secrets are that environment's. A
   pull request's dry run of `release.yml` never sees them and builds
   unsigned.
2. **Signed before it is counted.** The signing jobs sit between `build` and
   `publish` and replace the built files; `SHA256SUMS` and the build
   provenance attestation are made over the signed files.
3. **macOS:** `sign-macos` signs `veilq` with the Developer ID (hardened
   runtime, timestamp) and notarizes it with `notarytool` and an App Store
   Connect API key. A bare binary cannot carry its ticket (no staple);
   Gatekeeper looks it up online. Secrets: `MACOS_CERTIFICATE_P12`
   (base64), `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGNING_IDENTITY`,
   `MACOS_NOTARY_KEY_P8`, `MACOS_NOTARY_KEY_ID`, `MACOS_NOTARY_ISSUER`.
4. **Windows:** `check-windows` reads `veilq.exe`'s Authenticode signature
   (`Get-AuthenticodeSignature`). The signing step comes with the provider:
   each needs its own tool, and none is built on suspicion.
5. **Fail closed once switched on.** The repository variables `SIGN_MACOS`
   and `SIGN_WINDOWS` switch signing on per platform; then a release without
   a valid signature (or with a secret missing) fails. While a switch is
   off, the release goes out unsigned as before, and its notes say which
   platform is not signed.
6. **What a signature claims:** that nuetzliches' release workflow built
   the file from the tag, the same as the attestation, in a form the
   operating system and virus scanners read. It is no review of the code;
   the README says so and how to check a signature.

## Alternatives considered

- **Signing in the build job:** the build job also runs for pull requests;
  an environment there would hold every dry run for approval, or put the
  secrets within reach of a branch.
- **Signing after `publish`:** the checksums and the attestation would name
  files nobody downloads.
- **A Windows step for every provider now:** three tools to maintain, two of
  them never used.

## Consequences

- Before the first signed release, the owner creates the environment
  `release` (with a protection rule if wanted), puts the secrets in, and sets
  the variable. With a variable on and a secret missing, the release fails
  and says which.
- Choosing the Windows provider adds one step to a `sign-windows` job before
  `check-windows`; the switch and the check are there already.

## Enforced by

- `.github/workflows/release.yml` (`sign-macos`, `check-windows`, `publish`)
- `README.md` (Install → Signatures)
