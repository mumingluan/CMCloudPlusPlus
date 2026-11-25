# AxuanStor integration status

The shared Rust SDK dependency is `../../AxuanStor/sdk/rust` from `src-tauri`.
Repository discovery/navigation, SDK upload plans, plain-repository cloud in/out
relocation and verified exports are implemented. Windows native repository
tests, TypeScript checks, frontend interaction tests and the scoped visual review
have passed. The production application has not been replaced by these builds.

## System secret storage

`src-tauri/src/system_secrets.rs` provides explicit opt-in retention for
repository unlock material. Windows uses
the existing DPAPI primitive, with an authenticated logical-entry envelope.
Android's native bridge uses a non-exportable Android Keystore AES-256-GCM key,
random IVs and entry-specific AAD. The bridge returns opaque sealed bytes for
local storage; it does not export its wrapping key. Device hardware backing is
not assumed. Platforms without this implementation return an unsupported error
instead of using an application-directory wrapping key for these secrets.

Discovery badges, default repository navigation and the ordinary-folder override
share the existing file browser. Opaque native repository handles keep unlocks
out of React state. Upload plans hash the complete selected batch, compare
repository and intra-batch content, then recheck source identity before publish.
SDK metadata preserves concurrent histories and persists observed heads across
independent sessions and restart. Plain repositories also support cloud-side
copy and move into and out of ordinary Mobile Cloud folders without payload
downloads or re-uploads.

The preexisting account vault remains separate. Its credentials and session data
have not been read, migrated or changed during implementation. Repository
passwords and key bytes must stay in native session memory after unlock; the UI
receives an opaque repository handle and saved/not-saved status. Saving a key
means an OS-protected local copy, not an unencrypted duplicate key file.

Windows's DPAPI round-trip, copied-entry/context refusal and explicit removal
test passes using a temporary database under R:/Temp. The Android plugin's
Kotlin compilation also passes with isolated Gradle outputs under R:/Temp.
Actual Android Keystore/device acceptance remains required before claiming
on-device storage verification.

Provider credentials and repository unlocks remain separate authorities and are
excluded from status/log output.

## Production boundary

The GUI runtime and preexisting account vault were not replaced. Implementation
builds and test outputs stay under R:/Temp. Separately, the authorized AxuanStor
architecture migration upgraded its original coordinator/four worker services
and renamed the same independent cloud repository to x3client-hotfix.

The explicit ignored native test original_repository_native_read_only passed
against that real repository using an independent provider credential and private
observation directory. It discovered/opened the repository, checked the adopted
188484-path metadata snapshot and original object identity without using a
coordinator, existing vault, cloud mutations or archive payload downloads. That
snapshot includes completed Package paths, controls and validation paths; it is
not a claim that every Package path is already archived. The test is opt-in and
must not run against an arbitrary account during ordinary unit tests.
