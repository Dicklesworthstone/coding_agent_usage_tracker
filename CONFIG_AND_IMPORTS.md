# `caut` Config & Imports

This document defines `caut`’s **native config format**, defaults, and how the CodexBar import works.
It is part of the port spec and should be treated as source‑of‑truth for CLI behavior.

---

## 1) Config Location & Precedence

### Paths
- **Config file (default):** `~/.config/caut/config.toml`
- **Config dir override:** `CAUT_CONFIG_DIR`
- **Config file override:** `CAUT_CONFIG_PATH` (highest priority)

### Precedence
1. **CLI flags** (always highest priority)
2. **Config file**
3. **Built‑in defaults**

---

## 2) Config File Format (TOML)

### Top‑Level Schema (v1)

```toml
schema_version = "caut.v1"

[providers]
# If set, these lists are authoritative.
enabled = ["codex", "claude"]
disabled = ["cursor", "factory"]

[output]
# Default output for `usage` when no --format or --robot specified.
default_format = "human" # human | json | md
no_color = false
pretty_json = false

[display]
# Countdown or absolute reset display.
reset_time = "countdown" # countdown | absolute

[fetch]
# Global defaults; per‑provider overrides may be added later.
source_mode = "auto" # auto | web | cli | oauth
web_timeout_seconds = 60
```

### Notes
- `providers.enabled` + `providers.disabled` are **optional**.
  - If **both empty**, `caut` uses built‑in defaults (all providers enabled unless excluded by platform).
  - If **either is non‑empty**, the lists are authoritative:
    - `enabled` wins for allow‑list semantics.
    - `disabled` removes entries from `enabled` if both are present.
- `output.default_format` controls **human vs robot defaults** (JSON is robot default; Markdown is optional).
- `display.reset_time` mirrors CodexBar’s `resetTimesShowAbsolute`.
- `fetch.source_mode` is a **default** only; providers may still reject unsupported modes.

---

## 3) Built‑In Defaults

```text
providers.enabled   = []            # means “use builtin provider defaults”
providers.disabled  = []
output.default_format = "human"
output.no_color     = false
output.pretty_json  = false
display.reset_time  = "countdown"
fetch.source_mode   = "auto"
fetch.web_timeout_seconds = 60
```

---

## 4) CodexBar Import

### Command
```
caut config import codexbar
```

### What it reads (macOS only)
From UserDefaults domains:
1) `com.steipete.codexbar`
2) `com.steipete.codexbar.debug` (used if the first is empty)

Keys:
- `providerToggles`: dictionary of `cliName -> Bool`
- `resetTimesShowAbsolute`: Bool

### Mapping → `caut` config

| CodexBar Key | `caut` Field | Mapping |
| --- | --- | --- |
| `providerToggles` | `providers.enabled` / `providers.disabled` | `true` → enabled, `false` → disabled |
| `resetTimesShowAbsolute` | `display.reset_time` | `true` → `absolute`, `false` → `countdown` |

### Behavior
- Import **writes** `~/.config/caut/config.toml` (or the overridden path).
- If the file already exists, import **merges** (does not erase unrelated config keys).
- On non‑macOS, the command exits with a clear message: import not supported.

---

## 5) Token Accounts Import (separate from config)

Token accounts are **not** in config; they are stored in a JSON file and handled by:

```
caut token-accounts convert --from codexbar --to caut
```

This preserves multi‑account labels, active index, and provider mapping.

---

## 6) Validation Rules

- `schema_version` must be `caut.v1`.
- `providers.enabled` / `providers.disabled` entries must match known provider IDs.
- `output.default_format` must be `human`, `json`, or `md`.
- `display.reset_time` must be `countdown` or `absolute`.
- `fetch.source_mode` must be `auto`, `web`, `cli`, or `oauth`.

On validation failure: emit a single, clear error and exit `1`.

