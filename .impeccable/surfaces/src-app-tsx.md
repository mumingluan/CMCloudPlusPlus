---
version: 1
slug: "src-app-tsx"
primary_target: "src/App.tsx"
related_targets: ["src/styles.css", "src/AccountView.tsx"]
---

# File workspace

Mode: Operate. Scope: React frontend inside Tauri 2, Windows and Android.

User chose the blue file workspace with light, dark, and system themes and code-first delivery. The primary job is reliable batch transfers and file management at TB/file and 10k-file scale.

## Direction contract

THESIS: An orderly file desk with a separately inspectable transfer ledger makes large operations understandable.

OWN-WORLD: Cool white working area, slate-tinted navigation, precise blue actions, compact sans text, crisp rows and restrained seams.

STORY: Connect an account, choose a cloud space, find files, manage them, and inspect transfer progress and recovery.

FIRST VIEWPORT: 218px collapsible navigation, generous file heading, compact action bar, dominant virtual file table; optional 260px details panel. Mobile uses a 56px navigation rail and a single column. Selection reveals batch actions.

FORM: File desk. Signature interaction: selected rows become a persistent batch toolbar.

FINISH: The five scoped interface fixes and their regressions received a ship verdict. DESIGN.md captures the implemented visual system; shipping raster assets carry provenance. Cloud service and native platform verification are recorded in docs/VERIFICATION.md.

## Interaction principles

Keep persistent transfer states legible, virtualize large file lists, and use aligned tabular figures for progress. Mobile operations follow a single-column layout. Previews support file management. The surface uses code-native icons and components.

Account tasks use a single-column workspace: current connection, explicit space confirmation, and a disclosed sign-in form. Session management and settings are pinned to the bottom of the navigation rail. Saved identities appear once; current space controls and the add-account form sit below the list.

The transfer workspace owns temporary upload hash controls, reusing the existing blue tab buttons in a flat row. File changes appear after confirmed operations and reconcile without discarding loaded pages.
