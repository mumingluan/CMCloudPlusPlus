# CMCloud++

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Tauri 2 native shell and Rust service layer; React + TypeScript frontend, recommended in the interview. Windows and Android are primary targets, with an iOS-compatible project.

## Users

Chinese-speaking China Mobile cloud users managing files and reliable batch transfers on desktop and mobile.

## Product Purpose

Provide an efficient, stable, capable third-party China Mobile cloud client with the local OpenList 139 driver capabilities for the supported spaces.

## Operating Context

The protocol reference is OpenListTeam/OpenList, drivers/139. Desktop supports explicitly importing credentials from the installed official client.

## Capabilities and Constraints

Three spaces: personal, group, and shared links. Preserve the reference driver's Authorization, account/password, mail-cookie, and risk-control SMS continuation login methods. File management and reliable batch transfers are the confirmed first priority. Android SDK is available; iOS builds require a macOS environment. Provider changes and account-specific support require live verification.

## Product Principles

- Preserve credentials in the native layer and expose only masked account summaries to the interface.
- Make transfer state and recovery visible and actionable.
- Keep desktop file operations efficient and mobile controls reachable.
- Distinguish verified provider behavior from local tests and unverified capabilities.

## Evidence on Hand

Local OpenList Go source and tests, and official Windows client data. No remote cloud changes are needed to inspect local integration.

## Open Decisions

The working application name CMCloud++ is inferred from the workspace. It is a third-party client. No official brand assets or affiliation have been provided.
