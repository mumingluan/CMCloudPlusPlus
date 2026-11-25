# File workspace

Mode: Operate. Scope: React frontend inside Tauri 2, Windows and Android.

User chose the bright blue file workspace and code-first delivery. The primary job is reliable batch transfers and file management at TB/file and 10k-file scale.

## Direction contract

THESIS: An orderly file desk with a separately inspectable transfer ledger makes large operations understandable.

OWN-WORLD: Cool white working area, slate-tinted navigation, precise blue actions, compact sans text, crisp rows and restrained seams.

STORY: Connect an account, choose a cloud space, find files, manage them, and inspect transfer progress and recovery.

FIRST VIEWPORT: 218px collapsible desktop navigation, compact action bar and a dominant shared virtual file list or icon grid. Mobile uses fixed bottom navigation and a single column. Selection reveals batch actions; repository browsing adds Manage repository / Exit repository to the same toolbar.

FORM: File desk. Signature interaction: selected rows become a persistent batch toolbar.

FINISH: Earlier scoped interface fixes retain their recorded verdicts. The 2026-11-11 reviewer scored destructive-action contrast, shared mobile operation targets, demo disclosure and persistence resolved, with **ship** covering those four fixes and attributable regressions at React/headless-Chromium fixture scope. DESIGN.md captures the built system; provider and native platform verification remain bounded by docs/VERIFICATION.md.

## Interaction principles

Keep persistent transfer states legible, virtualize large file lists, and use aligned tabular figures for progress. Mobile operations follow a single-column layout. Previews support file management. The surface uses code-native icons and components.

## Repository integration

Ordinary folders and repository logical files share App.tsx's list/grid, selection, search, sorting, filtering, context menu and OperationDialog. RepositoryWorkspace.tsx controls repository sessions and dialogs. Repository transfers enter the existing queue; encryption/decryption and preview preparation use its visible stages and recovery controls. Inside a repository, New repository is hidden and Manage repository / Exit repository remain in the shared toolbar.

AxuanStor settings provides per-repository cache name, ID, size and deletion, with an in-use guard. Removing saved repository passwords, keys and coordinator tokens requires two confirmations. Decrypted preview sources are temporary, disable copying their local link, and request release on close. Browser fixture sessions have a persistent demo disclosure, with labels inside management and upload-plan dialogs.

The current evidence is headless Chromium desktop-light / phone-dark fixture review at R:/Temp/CMCloud-Unified-20261111/ui-review. The [scoped final verdict](R:/Temp/CMCloud-Unified-20261111/ui-review/reviewer-verdict.md) is **ship** for the four listed fixes and their attributable regressions. Native Windows GUI acceptance was postponed by the user; physical Android, Keystore / native security storage, provider operations and security acceptance remain separate.
