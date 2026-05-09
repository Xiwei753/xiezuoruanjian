# Writer App

A robust, local-first, cross-platform writing application designed for long-term productivity, safety, and extensibility.

## Core Philosophy

1.  **Zero Data Loss:** Absolute safety for user manuscripts. The underlying storage mechanism prevents concurrent writing corruption and uses OS-level atomic renames.
2.  **Local-First & Open Formats:** The primary source of truth is the local file system using open formats (Markdown for content, JSON for metadata). SQLite is strictly used as a transient index/cache that can be entirely rebuilt from the file system.
3.  **Clean Architecture:** Strict separation of concerns. The UI layer knows nothing about the file system or databases. External systems (AI, Git Sync) are completely abstracted.

---

## Codebase Architecture & File Guide

This project strictly adheres to **Clean Architecture** principles. Below is a guide to the foundational files introduced in the MVP stage. This will help you navigate the repository without having to read every single file.

### 1. Domain Layer (`lib/domain/`)
The absolute core of the application. Contains pure business logic, entities, and abstract interfaces. **It has zero dependencies on any other layer or external libraries (like Flutter UI or file system APIs).**

*   **Models:**
    *   `lib/domain/models/project.dart`: The `Project` entity representing a novel or major work. Uses immutable state (`copyWith`).
    *   `lib/domain/models/chapter.dart`: The `Chapter` entity representing a single chapter. Includes a `contentHash` for sync tracking and conflict detection.
*   **Repository Interfaces:**
    *   `lib/domain/repositories/chapter_repository.dart`: Abstract interface (`IChapterRepository`) defining how chapters are saved and how the cache index is rebuilt. Hides the complexity of interacting with the storage and cache layers.
*   **Service Interfaces:**
    *   `lib/domain/services_interfaces/storage_service.dart`: Interface (`IStorageService`) for raw file writing operations.
    *   `lib/domain/services_interfaces/sync_service.dart`: Interface (`ISyncService` and `IGitClient`) defining non-destructive synchronization flows, explicitly highlighting conflict handling.
    *   `lib/domain/services_interfaces/ai_provider.dart`: Interface (`IAIProvider`) for any AI integrations (e.g., DeepSeek). Ensures AI logic remains decoupled and cancellable.
    *   `lib/domain/services_interfaces/correction_engine.dart`: Interface (`ICorrectionEngine`) for local text correction features.

### 2. Application Layer (`lib/application/`)
Orchestrates the domain objects to perform actual use cases. Connects the UI to the underlying infrastructure via interfaces.

*   **Background Tasks:**
    *   `lib/application/background_tasks/file_write_queue.dart`: **Critical safety file.** Implements `FileWriteQueue`. It debounces and forces all file save operations to process sequentially. This guarantees that rapid autosaves will never result in concurrent file write corruption.

### 3. Infrastructure Layer (`lib/infrastructure/`)
The lowest level. Contains the concrete implementations for databases, file systems, network calls, etc.

*   **Storage:**
    *   `lib/infrastructure/storage/atomic_writer.dart`: **Critical safety file.** Implements `IStorageService`. It writes data to a `.tmp` file first, and upon success, performs an OS-level atomic `rename()` to overwrite the actual file. If a crash occurs mid-write, the original file remains untouched.
*   **Database (Cache):**
    *   `lib/infrastructure/database/database_helper.dart`: The SQLite connection stub. Outlines the tables needed (`projects_cache`, `chapters_cache`) and documents the crucial requirement that the entire database can be destroyed and rebuilt from the `workspacePath` at any time.

### 4. Presentation / UI Layer (`lib/presentation/` & `lib/main.dart`)
*(Currently empty outside of default Flutter scaffolding).*
This layer will contain Flutter widgets and state management (e.g., Riverpod). It will strictly consume `UseCases` from the Application layer and will never instantiate Infrastructure classes directly.

---

## Getting Started

1. Ensure you have Flutter installed (targeting Linux and Android).
2. Run `flutter pub get`.
3. Run `flutter test` to verify domain logic.
4. Run `flutter run` (Currently displays the default Flutter scaffolding).
