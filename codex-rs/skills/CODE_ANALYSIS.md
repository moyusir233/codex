# CODE_ANALYSIS

## Scope

This document analyzes the `codex-skills` crate at
[/Users/bytedance/project/codex/codex-rs/skills](file:///Users/bytedance/project/codex/codex-rs/skills).
The crate is small and focused: it owns the embedding and installation of a
set of bundled “system skills” into the user’s skills directory, and exposes
just enough API for the wider workspace to locate and manage those cached
bundled skills.

The analysis covers:

- responsibilities and public API
- control flow and data flow
- dependencies (internal and external)
- tests and invariants
- design choices and tradeoffs
- open questions and potential improvements

## Crate Role

At a high level, `codex-skills` is a support crate for Codex’s skill system:

- It **embeds a tree of sample/system skills** from
  [`src/assets/samples`](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples)
  directly into the binary at compile time.
- It **materializes those embedded skills on disk** under
  `$CODEX_HOME/skills/.system` on startup when bundled skills are enabled.
- It **fingerprints the embedded contents** to avoid rewriting the cache when
  nothing changed.
- It **exposes a small public API** used by `core-skills` to:
  - compute the on-disk cache directory for system skills
  - install (or refresh) the embedded skills into that directory

The crate deliberately does not implement general skill discovery or loading.
That logic lives in `codex-core-skills`; this crate only handles the embedded
system skills cache.

## Manifest and Structure

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/skills/Cargo.toml#L1-L19):

- Package:
  - `name = "codex-skills"`
  - `edition`, `license`, `version` inherited from the workspace.
  - `build = "build.rs"`: custom build script to hook change detection for
    embedded assets.
- Library:
  - `name = "codex_skills"`
  - `path = "src/lib.rs"`
  - doctests are disabled.
- Dependencies (workspace-managed versions):
  - `codex-utils-absolute-path`: absolute-path wrapper type used widely in the
    workspace.
  - `include_dir`: compile-time directory embedding and runtime traversal.
  - `thiserror`: ergonomic error type derivation.

Source layout:

- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L1-L169):
  - main crate implementation and public API.
  - embeds the system skills directory.
  - fingerprinting and installation logic.
  - error type definition.
  - one unit test for fingerprint coverage.
- [src/assets/samples](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples):
  - tree of embedded “system skills” shipped with Codex, e.g.:
    - `skill-creator`
    - `skill-installer`
    - `openai-docs`
    - `imagegen`
- [build.rs](file:///Users/bytedance/project/codex/codex-rs/skills/build.rs#L1-L27):
  - build script that ensures changes under `src/assets/samples` trigger a
    rebuild.

The Bazel target
[`BUILD.bazel`](file:///Users/bytedance/project/codex/codex-rs/skills/BUILD.bazel#L1-L15)
declares `codex_skills` as a `codex_rust_crate` and includes all files as
compile-time data, which is consistent with using `include_dir`.

## Public API and Responsibilities

### Constants and Embedded Directory

In [lib.rs](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L1-L16):

- `const SYSTEM_SKILLS_DIR: Dir`:
  - defined by `include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/assets/samples")`.
  - at compile time, captures the full subtree under `src/assets/samples`
    (directories, SKILL.md, scripts, references, assets, etc.).
  - at runtime, provides a read-only `include_dir::Dir` view over the embedded
    data.

- `const SYSTEM_SKILLS_DIR_NAME: &str = ".system";`
- `const SKILLS_DIR_NAME: &str = "skills";`
- `const SYSTEM_SKILLS_MARKER_FILENAME: &str = ".codex-system-skills.marker";`
- `const SYSTEM_SKILLS_MARKER_SALT: &str = "v1";`

These constants define the on-disk layout and fingerprinting metadata:

- System skills are cached under `$CODEX_HOME/skills/.system`.
- A marker file at
  `$CODEX_HOME/skills/.system/.codex-system-skills.marker` stores the
  fingerprint for the currently installed embedded skills.
- The `SYSTEM_SKILLS_MARKER_SALT` is mixed into the fingerprint to make it
  easy to invalidate the cache independent of file contents (e.g., if the
  fingerprinting algorithm changes).

### `system_cache_root_dir`

Signature and implementation:

- [lib.rs:L17-L22](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L17-L22)

```rust
pub fn system_cache_root_dir(codex_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    codex_home
        .join(SKILLS_DIR_NAME)
        .join(SYSTEM_SKILLS_DIR_NAME)
}
```

Responsibility:

- Compute the canonical on-disk directory where embedded system skills are
  (or will be) cached for a given `CODEX_HOME`.
- It is purely a deterministic path computation; it does not touch the
  filesystem.

Downstream usage:

- `core-skills/src/system.rs` re-exports this function:
  - [system.rs](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/system.rs#L1-L7)
  - Used to:
    - compute the path for uninstalling system skills:
      `uninstall_system_skills` calls
      `std::fs::remove_dir_all(system_cache_root_dir(codex_home))`.
    - construct a `SkillRoot` for system skills in the loader:
      [loader.rs:L286-L292](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/loader.rs#L286-L292).

### `install_system_skills`

Signature and high-level behavior:

- [lib.rs:L24-L56](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L24-L56)

```rust
pub fn install_system_skills(codex_home: &AbsolutePathBuf) -> Result<(), SystemSkillsError> {
    let skills_root_dir = codex_home.join(SKILLS_DIR_NAME);
    fs::create_dir_all(skills_root_dir.as_path())
        .map_err(|source| SystemSkillsError::io("create skills root dir", source))?;

    let dest_system = system_cache_root_dir(codex_home);

    let marker_path = dest_system.join(SYSTEM_SKILLS_MARKER_FILENAME);
    let expected_fingerprint = embedded_system_skills_fingerprint();
    if dest_system.as_path().is_dir()
        && read_marker(&marker_path).is_ok_and(|marker| marker == expected_fingerprint)
    {
        return Ok(());
    }

    if dest_system.as_path().exists() {
        fs::remove_dir_all(dest_system.as_path())
            .map_err(|source| SystemSkillsError::io("remove existing system skills dir", source))?;
    }

    write_embedded_dir(&SYSTEM_SKILLS_DIR, &dest_system)?;
    fs::write(marker_path.as_path(), format!("{expected_fingerprint}\n"))
        .map_err(|source| SystemSkillsError::io("write system skills marker", source))?;
    Ok(())
}
```

Key responsibilities:

- Ensure `$CODEX_HOME/skills` exists as a directory.
- Compute the destination directory `$CODEX_HOME/skills/.system`.
- Compute the fingerprint of the embedded skill tree:
  `embedded_system_skills_fingerprint()`.
- **Skip reinstall** if:
  - `$CODEX_HOME/skills/.system` exists and is a directory, and
  - the marker file exists and its trimmed contents equal the expected
    fingerprint.
- Otherwise:
  - delete any existing `$CODEX_HOME/skills/.system` directory.
  - write the entire embedded directory tree to disk at that location via
    `write_embedded_dir`.
  - write the marker file with the new fingerprint.

This function is the only public entry point that mutates the filesystem for
system skills. It is called by `core-skills` on manager construction.

Downstream integration:

- In `core-skills/src/system.rs`, the function is re-exported:
  - [system.rs:L1-L2](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/system.rs#L1-L2)
- In `core-skills/src/manager.rs`, it is invoked as part of skill-manager
  construction:
  - [manager.rs:L62-L79](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/manager.rs#L62-L79)

```rust
pub fn new_with_restriction_product(
    codex_home: AbsolutePathBuf,
    bundled_skills_enabled: bool,
    restriction_product: Option<Product>,
) -> Self {
    let manager = Self { /* fields elided */ };
    if !bundled_skills_enabled {
        // The loader caches bundled skills under `skills/.system`. Clearing that directory is
        // best-effort cleanup; root selection still enforces the config even if removal fails.
        uninstall_system_skills(&manager.codex_home);
    } else if let Err(err) = install_system_skills(&manager.codex_home) {
        tracing::error!("failed to install system skills: {err}");
    }
    manager
}
```

Behavioral contract:

- If `bundled_skills_enabled` is true, Codex will attempt to ensure that
  `$CODEX_HOME/skills/.system` contains a fresh copy of the embedded skills.
- Errors from installation are logged but do not prevent the manager from
  being constructed; downstream skill-loading respects configuration flags
  and may operate without system skills.

### `SystemSkillsError`

- Defined in [lib.rs:L130-L144](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L130-L144).

```rust
#[derive(Debug, Error)]
pub enum SystemSkillsError {
    #[error("io error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
}

impl SystemSkillsError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}
```

Responsibilities:

- Wrap IO failures with context describing the high-level action:
  - `"create skills root dir"`
  - `"remove existing system skills dir"`
  - `"write system skills marker"`
  - `"create system skills dir"`
  - `"create system skills subdir"`
  - `"create system skills file parent"`
  - `"write system skill file"`
  - `"read system skills marker"`
- Enable callers to log structured errors via `Display` and `source()`.

This error type is internal to the crate and callers (`core-skills`) handle it
by logging and continuing.

## Internal Flow and Implementation Details

### Fingerprinting the Embedded Directory

The fingerprint logic lives in
[lib.rs:L65-L76](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L65-L76)
and
[lib.rs:L79-L96](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L79-L96).

```rust
fn embedded_system_skills_fingerprint() -> String {
    let mut items = Vec::new();
    collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut items);
    items.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

    let mut hasher = DefaultHasher::new();
    SYSTEM_SKILLS_MARKER_SALT.hash(&mut hasher);
    for (path, contents_hash) in items {
        path.hash(&mut hasher);
        contents_hash.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}
```

`collect_fingerprint_items` walks the embedded directory:

```rust
fn collect_fingerprint_items(dir: &Dir<'_>, items: &mut Vec<(String, Option<u64>)>) {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                items.push((subdir.path().to_string_lossy().to_string(), None));
                collect_fingerprint_items(subdir, items);
            }
            include_dir::DirEntry::File(file) => {
                let mut file_hasher = DefaultHasher::new();
                file.contents().hash(&mut file_hasher);
                items.push((
                    file.path().to_string_lossy().to_string(),
                    Some(file_hasher.finish()),
                ));
            }
        }
    }
}
```

Data flow:

- Traverse the embedded directory tree.
- For each directory:
  - push `(path_string, None)` into `items`.
- For each file:
  - compute a per-file `u64` hash of the file contents using `DefaultHasher`.
  - push `(path_string, Some(contents_hash))` into `items`.
- After traversal:
  - sort by `path_string` to make the final hash deterministic regardless of
    iteration order.
  - hash:
    - the `SYSTEM_SKILLS_MARKER_SALT`
    - each `path_string`
    - each `contents_hash` (or its `None`).
- Produce a lower-hex string from the final `u64` hash.

Invariants:

- Any change to:
  - directory structure (paths),
  - file contents,
  - or the salt
  will change the fingerprint and therefore force a reinstall on the next
  `install_system_skills` call.

### Marker File Handling

`read_marker` lives at
[lib.rs:L58-L63](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L58-L63):

```rust
fn read_marker(path: &AbsolutePathBuf) -> Result<String, SystemSkillsError> {
    Ok(fs::read_to_string(path.as_path())
        .map_err(|source| SystemSkillsError::io("read system skills marker", source))?
        .trim()
        .to_string())
}
```

Behavior:

- Reads the marker file as UTF-8 text.
- Trims whitespace and returns the fingerprint string.
- On failure, returns `SystemSkillsError::Io` with `"read system skills marker"`.

Call site behavior:

- In `install_system_skills` the marker is only used as a hint:

```rust
if dest_system.as_path().is_dir()
    && read_marker(&marker_path).is_ok_and(|marker| marker == expected_fingerprint)
{
    return Ok(());
}
```

- Any failure to read or parse the marker causes the function to fall through
  and reinstall the embedded directory. This makes the marker effectively a
  cache optimization with safe fallback behavior.

### Writing the Embedded Directory

Implementation:

- [lib.rs:L98-L128](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L98-L128)

```rust
fn write_embedded_dir(dir: &Dir<'_>, dest: &AbsolutePathBuf) -> Result<(), SystemSkillsError> {
    fs::create_dir_all(dest.as_path())
        .map_err(|source| SystemSkillsError::io("create system skills dir", source))?;

    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                let subdir_dest = dest.join(subdir.path());
                fs::create_dir_all(subdir_dest.as_path()).map_err(|source| {
                    SystemSkillsError::io("create system skills subdir", source)
                })?;
                write_embedded_dir(subdir, dest)?;
            }
            include_dir::DirEntry::File(file) => {
                let path = dest.join(file.path());
                if let Some(parent) = path.as_path().parent() {
                    fs::create_dir_all(parent).map_err(|source| {
                        SystemSkillsError::io("create system skills file parent", source)
                    })?;
                }
                fs::write(path.as_path(), file.contents())
                    .map_err(|source| SystemSkillsError::io("write system skill file", source))?;
            }
        }
    }

    Ok(())
}
```

Responsibilities:

- Create the destination root directory if it does not exist.
- For each embedded directory:
  - create the corresponding directory on disk.
  - recurse into the directory to write its descendants.
- For each embedded file:
  - ensure the parent directory exists.
  - write the file contents to disk.

Note on implementation detail:

- The recursive call `write_embedded_dir(subdir, dest)` uses the original
  `dest` rather than `subdir_dest`. Because file paths are joined via
  `dest.join(file.path())`, this still produces the correct final location.
  However, the recursive structure effectively re-walks the subtree using
  root-relative paths for each file. This is correct but potentially
  redundant; see “Open Questions” below.

Error semantics:

- Any IO error at directory creation or file write time returns a
  `SystemSkillsError::Io` with contextual `action`, allowing the caller to
  log what failed in which phase.

### Build Script and Rebuild Triggers

[build.rs](file:///Users/bytedance/project/codex/codex-rs/skills/build.rs#L1-L27)
implements a simple directory walk:

```rust
fn main() {
    let samples_dir = Path::new("src/assets/samples");
    if !samples_dir.exists() {
        return;
    }

    println!("cargo:rerun-if-changed={}", samples_dir.display());
    visit_dir(samples_dir);
}

fn visit_dir(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            visit_dir(&path);
        }
    }
}
```

Responsibilities:

- Ensure that any change to:
  - the root `src/assets/samples` directory, or
  - any nested file or directory beneath it
  triggers a rebuild of the crate.

This is important because:

- `SYSTEM_SKILLS_DIR` is a compile-time embedded directory. If the files on
  disk change but Rust does not rebuild the crate, the binary would still
  contain stale embedded contents.

## Embedded Skills Contents

The embedded directory
[`src/assets/samples`](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples)
contains several complete skills, each following the standard skill layout
described in the `skill-creator` skill itself:

```text
skill-name/
├── SKILL.md
├── agents/
│   └── openai.yaml
├── scripts/
├── references/
└── assets/
```

Key examples:

- `skill-creator`:
  - [SKILL.md](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples/skill-creator/SKILL.md#L1-L415)
  - Comprehensive guide to designing, initializing, validating, and iterating
    skills. Defines the canonical layout and principles (concise context,
    progressive disclosure, naming rules, etc.).
- `skill-installer`:
  - [SKILL.md](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples/skill-installer/SKILL.md#L1-L58)
  - Scripts:
    - `scripts/list-skills.py`
    - `scripts/install-skill-from-github.py`
  - Provides workflows for listing curated skills and installing them from
    GitHub into `$CODEX_HOME/skills`.
- `openai-docs`:
  - [SKILL.md](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples/openai-docs/SKILL.md#L1-L69)
  - Leans on the OpenAI developer docs MCP server; bundled references explain
    GPT-5.4 migration and model selection.
- `imagegen`:
  - [SKILL.md](file:///Users/bytedance/project/codex/codex-rs/skills/src/assets/samples/imagegen/SKILL.md#L1-L279)
  - Scripts:
    - `scripts/image_gen.py`
  - Encapsulates both the built-in `image_gen` tool path and an explicit CLI
    fallback path for image generation/editing, with detailed prompt
    guidance and sandbox/network notes.

From this crate’s point of view, these are opaque assets; they are simply
embedded and written to disk. However, they heavily shape the Codex skill
ecosystem, as they ship as built-in skills in every Codex installation.

## Testing

The crate’s tests live in the `tests` module at the bottom of
[lib.rs](file:///Users/bytedance/project/codex/codex-rs/skills/src/lib.rs#L146-L169).

### `fingerprint_traverses_nested_entries`

Implementation:

```rust
#[cfg(test)]
mod tests {
    use super::SYSTEM_SKILLS_DIR;
    use super::collect_fingerprint_items;

    #[test]
    fn fingerprint_traverses_nested_entries() {
        let mut items = Vec::new();
        collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut items);
        let mut paths: Vec<String> = items.into_iter().map(|(path, _)| path).collect();
        paths.sort_unstable();

        assert!(
            paths
                .binary_search_by(|probe| probe.as_str().cmp("skill-creator/SKILL.md"))
                .is_ok()
        );
        assert!(
            paths
                .binary_search_by(|probe| probe.as_str().cmp("skill-creator/scripts/init_skill.py"))
                .is_ok()
        );
    }
}
```

Validation:

- Confirms that:
  - `collect_fingerprint_items` traverses nested directories and files under
    `SYSTEM_SKILLS_DIR`.
  - Specific known paths (a top-level SKILL.md and a nested script) appear in
    the collected items.
- This test implicitly asserts that:
  - the embedded directory is wired up correctly,
  - the relative paths for embedded files match expectations.

Limitations:

- There are no direct tests for:
  - the fingerprint string itself,
  - behavior when the marker matches vs mismatches,
  - error propagation paths in `install_system_skills` and `write_embedded_dir`.
- Higher-level behavior (e.g., cleaning up stale `.system` directories, root
  selection) is exercised indirectly in `core-skills` tests such as
  [manager_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/manager_tests.rs#L315-L351).

## Design Considerations

### Separation of Concerns

- `codex-skills` is intentionally narrow:
  - It knows how to embed and materialize a bundled skill tree.
  - It does not know how to:
    - parse SKILL.md,
    - manage skill scope or precedence,
    - interpret config layers.
- `core-skills` owns:
  - skill discovery across multiple roots (repo, user, admin/system),
  - scope classification (`SkillScope::Repo`, `SkillScope::User`,
    `SkillScope::System`, etc.),
  - higher-level orchestration and caching.

This separation keeps the embedding/installation logic lightweight and
reusable while allowing `core-skills` to evolve discovery semantics without
pulling in the embedder.

### Fingerprint-Based Idempotency

- The system avoids rewriting `$CODEX_HOME/skills/.system` on every startup by
  comparing the current embedded fingerprint against a stored marker.
- Benefits:
  - avoids redundant IO when the bundled skills are unchanged,
  - allows safe reinstallation if the embedded skills change across versions,
  - simple implementation with no external state.
- The salt string provides a mechanism for globally invalidating existing
  markers without touching the filesystem layout; bumping `SYSTEM_SKILLS_MARKER_SALT`
  forces all installs to refresh once.

### Filesystem-Safe Behavior

- On install:
  - `$CODEX_HOME/skills` is created if needed.
  - If `.system` exists but has a mismatched or unreadable marker, the
    directory is removed and re-created.
- On uninstall:
  - `core-skills` calls `uninstall_system_skills`, which performs a best-effort
    `remove_dir_all` over the same path.
- This ensures:
  - no partial mixing of old and new embedded skills (directory is removed
    entirely before writing),
  - no risk of stale bundles when the embedder changes.

### Build-Time Embedding

- Using `include_dir` to embed the entire skills tree means:
  - No runtime dependency on the original `src/assets/samples` location.
  - Skills ship pre-packaged inside the binary/artifact.
  - Rebuilds are necessary when assets change; `build.rs` handles this via
    `cargo:rerun-if-changed`.
- This design aligns with the intent that “system skills” are part of the
  product, not user configuration.

## Open Questions and Improvement Ideas

These are questions or potential improvements surfaced during analysis, not
known bugs.

1. **Recursive `write_embedded_dir` destination parameter**
   - Current code recurses with `write_embedded_dir(subdir, dest)` but uses
     `dest.join(file.path())` for files, effectively treating `dest` as the
     root every time.
   - This is correct because `file.path()` is root-relative, but it makes the
     recursion structure somewhat misleading and may re-traverse path segments
     redundantly.
   - A more straightforward implementation might:
     - recurse with `write_embedded_dir(subdir, &subdir_dest)` and
     - use local relative paths under that directory.
   - Question: Is the current structure intentional (for simplicity with
     `include_dir` paths), or could it be simplified without changing the
     public API?

2. **Error handling and logging granularity**
   - `install_system_skills` returns a single `SystemSkillsError`, and
     `core-skills` logs a generic message on failure.
   - Question: Do we need more granular reporting at the call site (e.g.,
     can we surface whether failure occurred during marker read, directory
     cleanup, or file writes) for troubleshooting user environments?

3. **Testing coverage for marker/fingerprint behavior**
   - Only `collect_fingerprint_items` traversal is directly tested.
   - Behavior like “skip reinstall when marker matches” or “force reinstall
     when fingerprint changes” is currently covered implicitly via
     integration tests in `core-skills` (if at all).
   - Possible improvements:
     - Add unit tests that:
       - simulate a marker with matching vs mismatching fingerprint,
       - verify that `install_system_skills` avoids or performs rewrites
         accordingly (using a temporary filesystem or a mocked representation).

4. **Extensibility of embedded skills**
   - The current crate embeds a fixed set of skills under
     `src/assets/samples`.
   - Question: Do we anticipate:
     - multiple “channels” (e.g., `.system`, `.experiments`) of bundled
       skills,
     - or per-product subsets of system skills?
   - If yes, the crate might evolve to:
     - embed multiple roots,
     - expose installation APIs parameterized by channel or product, or
     - surface metadata about which skills are embedded.

5. **Runtime inspection of embedded skills**
   - Currently, consumers only see the installed on-disk view.
   - Question: Should `codex-skills` expose any introspection over the
     embedded directory (e.g., listing embedded skill names or metadata)
     without touching the filesystem?
   - This could help debugging or building diagnostic commands, but it would
     slightly expand the crate’s surface area.

6. **Interaction with user-managed skills**
   - System skills are installed under `.system` while user skills live under
     `$CODEX_HOME/skills/<name>` or `$HOME/.agents/skills`.
   - `skill-installer` SKILL.md notes that `.system` skills are preinstalled
     and generally should not be reinstalled from GitHub.
   - Open question: Should the crate enforce safeguards to prevent user
     overwrites of `.system` skills, or is that fully delegated to higher
     layers and user tooling?

Overall, `codex-skills` is a narrowly-scoped but central crate: it ensures
that every Codex installation has a consistent, versioned set of bundled
skills, while delegating discovery, precedence, and actual skill logic to
other crates and the skills themselves.

