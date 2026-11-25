---
version: 1
slug: "src-app-tsx"
primary_target: "src/App.tsx"
related_targets: ["src/styles.css", "src/AccountView.tsx", "src/RepositoryWorkspace.tsx", "src/RepositorySettings.tsx", "src/RepositoryDialog.tsx", "src/repositoryFiles.ts", "src/repositoryApi.ts", "src/OfflineDownloads.tsx", "src/repository.css", "src/offlineDownloads.css"]
---

# File workspace

Mode: Operate. Scope: React frontend inside Tauri 2, Windows and Android.

User chose the blue file workspace with light, dark, and system themes and code-first delivery. The primary job is reliable batch transfers and file management at TB/file and 10k-file scale.

## Direction contract

THESIS: An orderly file desk with a separately inspectable transfer ledger makes large operations understandable.

OWN-WORLD: Cool white working area, slate-tinted navigation, precise blue actions, compact sans text, crisp rows and restrained seams.

STORY: Connect an account, choose a cloud space, find files, manage them, and inspect transfer progress and recovery.

FIRST VIEWPORT: 218px collapsible desktop navigation, compact action bar and a dominant shared virtual file list or icon grid. Phones use fixed bottom navigation and a single column. Selection reveals batch actions; repository browsing adds Manage repository and Exit repository to the same toolbar.

FORM: File desk. Signature interaction: selected rows become a persistent batch toolbar.

FINISH: Earlier scoped interface fixes retain their recorded verdicts. The 2026-11-11 reviewer scored all four listed fixes resolved and returned **ship** for those fixes and attributable regressions at React/headless-Chromium fixture scope. Native Windows GUI remains deferred; physical Android, native security storage, cryptographic strength and actual provider operations require separate acceptance. DESIGN.md records the current built system; verification remains bounded by docs/VERIFICATION.md.

## Interaction principles

Keep persistent transfer states legible, virtualize large file lists, and use aligned tabular figures for progress. Mobile operations follow a single-column layout. Previews support file management. The surface uses code-native icons and components.

Desktop touch selection starts in blank file-list space and uses the same accent rectangle as mouse selection. Releasing preserves the selected rows and batch toolbar; touch starts on file rows retain native panning. The transparent background selection surface is mounted only outside the phone layout. Phones enter selection with a 450 ms file hold, cancel a pending hold after movement beyond 10px, and support continuous range selection from the checkboxes with edge scrolling.

The 2026-11-10 touch-selection repair received **ship** at the React/Chromium scope. The [scoped review and browser evidence](R:/Temp/CMCloud-Lazy-20261110/touch-browser/finish-review.md) cover desktop capture transfer and retained selection, phone panning and checkbox swipe. Native Windows GUI and physical Android behavior require separate acceptance.

Account tasks use a single-column workspace: current connection, explicit space confirmation, and a disclosed sign-in form. Session management and settings are pinned to the bottom of the navigation rail. Saved identities appear once; current space controls and the add-account form sit below the list.

The transfer workspace owns temporary upload hash controls, reusing the existing blue tab buttons in a flat row. File changes appear after confirmed operations and reconcile without discarding loaded pages.

## AxuanStor repository and offline additions

This is a code-led Operate extension of the established file desk. Light, dark, Follow System and the optional OLED pure-black appearance retain their existing behavior. All imagery is code-native; this addition introduces no raster assets. The existing direction contract and prior accepted interface fixes remain in force. The current implementation follows DESIGN.md's later compact-workspace update: file browsing starts at its toolbar, the desktop rail remains collapsible, and phones retain fixed bottom navigation.

In the personal space, confirmed repositories carry an inline blue A badge at the folder icon's lower right. Default open loads their logical files into the same App.tsx browser used for ordinary folders. The folder menu retains explicit repository and ordinary-folder open modes. App.tsx owns the shared list/icon grid, selection, search, filtering, sorting, context menu and OperationDialog; repositoryFiles.ts adapts repository data and submits mutations, including copy/move through batchEdit. RepositoryWorkspace.tsx owns session, unlock, management and upload-plan dialogs. Inside a repository, the shared toolbar hides New repository and offers Manage repository / Exit repository. Both contexts use 48px compact desktop rows, 72px comfortable/mobile rows and a 152px icon-grid row; Offline Download retains its own navigation destination.

Encrypted entry accepts a password, key text or imported key file. Remember/save starts unchecked and names system security storage; saved unlock information has explicit use and removal actions. The frontend exposes those choices; OS encryption, native persistence and provider behavior require their own acceptance.

The frontend presents a whole-selection upload plan with comparisons against existing content and within the batch. It distinguishes same-path skip, existing-content reuse, batch reuse and new upload, with logical bytes, new-content count and actual upload bytes. Fifty-item pagination affects inspection only. Replacement of existing paths requires separate consent.

Offline Download connects through URL and admin-token fields with an unchecked remember option, then dispatches one HTTP/HTTPS URL per line to a selected repository and logical directory. It renders task, node, repository, backend, batch and event state; task details disclose checksum, original-session presence, confirmed parts and attempt/history counts. Binary budgets use KiB/MiB/GiB and explicit per-second units. Loading, empty, error and retained-last-success states remain visible. Demo content is labelled. Native sanitization and actual coordinator/provider behavior remain unaccepted.

At widths up to 700px, all new buttons have at least 48px height and width; fields, remember labels, filters, pagination and task/node disclosure targets retain the 48px height floor. Desktop controls retain their compact sizes. Dialogs set initial focus once controls first become ready, without refocusing on later busy transitions. Their triggers are captured before opening; management and upload-plan return focus waits for asynchronous refresh and the shared toolbar to release busy state, with the enabled, labelled Manage repository action as the removed-trigger fallback. Shared OperationDialog footer actions also retain the 48 × 48px mobile floor.

Final frontend review: F1 (initial-ready and deferred-return focus) and F2 (mobile targets) were scored resolved, with disposition **ship**. The verdict covers those two corrections and attributable regressions in the supplied final captures. Backend work, native Windows/Android device and provider acceptance, and production migration remain unfinished. This is not whole-product or deployment acceptance. Evidence: [review packet](R:/Temp/AxuanStor-Architecture/ui-review/review-packet.md), [initial frontend review](R:/Temp/AxuanStor-Architecture/ui-review/reviewer-result.md), [final verdict](R:/Temp/AxuanStor-Architecture/ui-review/reviewer-verdict.md). All ten required screenshot paths now contain the final correction states, as confirmed by the reviewer.

### Repository creation and management

Writable personal spaces outside a repository expose New repository in the desktop toolbar and the phone Add menu. The compact single-column dialog collects a name and ordinary/encrypted type, starting with ordinary. Encrypted creation and credential changes use password/key controls: password confirmation reports mismatches in place; key entry accepts hexadecimal or Base64 text, imported files, and Generate and save new key. Imported/generated selections display the file name and pass an opaque selection identifier through the interface. Remember password / Save key on this device starts unchecked and names system security storage.

The folder three-dot menu includes Manage repository. It opens the repository, asks for unlock first when encrypted, then shows name, type, a wrapping ID in supporting text, and configured unlock methods. The shared file toolbar provides the same management route. Supported encrypted repositories expose Change password and Replace key, plus removal of saved local unlock information when present. The interface explains the early fixed-format migration requirement and the access boundary for unlocked devices, old keys and historical copies.

Creation, management and credential-change modals are capped at 440px and viewport width minus 32px, preserving the blue actions, restrained depth and 48px mobile controls. Submission, saved-information removal and pending import/generation keep their relevant controls disabled; the submitted method stays visible. Creation and management block dismissal during submission or a native picker request. Escape and app Back use the open modal's cancellation guard, and workspace return respects pending pickers. Settled requests restore the controls. Initial ordinary-repository loading retains a Cancel and return action and a retry after failure.

This extension's frontend evidence is the ten final desktop-light/mobile-dark captures, two pending-change captures and [pending-state evidence](R:/Temp/CMCloud-Repository-Work/ui-review/pending-evidence.json), alongside the [review packet](R:/Temp/CMCloud-Repository-Work/ui-review/review-packet.md) and [final verdict](R:/Temp/CMCloud-Repository-Work/ui-review/reviewer-verdict.md). The recorded behavior concerns the React interface; native key handling, provider behavior, Windows/Android device behavior and live performance require separate acceptance.

## Shared repository workspace (2026-11-11)

Repository uploads and downloads use the incumbent transfer queue, recovery controls and details. Encryption / decryption appear as queue stages. Whole-selection comparison still separates logical bytes, new content and upload bytes before enqueueing, with separate replacement consent. Those visible states do not establish byte-transform or cloud-write acceptance.

Settings adds an AxuanStor section using the existing flat preference rows. Its cache manager shows each repository's name, wrapping ID, size and Delete cache action; an in-use row explains why deletion is disabled. Phone records stack identity, size and action. Clearing saved repository passwords, keys and coordinator tokens requires two consecutive confirmations and uses the existing destructive button variant. Pending deletion locks its actions; errors and results remain in context.

Encrypted file preview uses the existing preview dialog and prepares a transient local source through the same transfer queue. The interface shows preparation or paused state, disables copying the transient link, and requests release when the preview closes. Native plaintext disposal, crash recovery and secret-store behavior need separate acceptance.

Browser preview fixtures retain a persistent “示例数据 · 界面预览” disclosure while browsing; management and upload-plan dialogs also label synthetic data. Real Tauri sessions do not show fixture disclosure. The 16 refreshed originals and evidence.json cover headless Chromium at desktop 1440 × 960 light and phone 390 × 844 dark. The reviewer scored destructive-action contrast, shared mobile operation targets, demo disclosure and persistence resolved, with disposition **ship** for these four fixes and their attributable regressions at React/headless-Chromium fixture scope.

Evidence: [review packet](R:/Temp/CMCloud-Unified-20261111/ui-review/review-packet.md), [initial review](R:/Temp/CMCloud-Unified-20261111/ui-review/reviewer-result.md), [current measured evidence](R:/Temp/CMCloud-Unified-20261111/ui-review/evidence.json), [scoped final verdict](R:/Temp/CMCloud-Unified-20261111/ui-review/reviewer-verdict.md). Native Windows GUI acceptance was postponed by the user while the desktop is locked. Physical Android, Android Keystore / system security storage, provider operations and security acceptance remain separate; this fixture review certifies none of them and makes no general cryptographic-strength claim.
