# Security Policy

## Supported Versions

AHA! Lang is under active development. Security updates are applied to the
latest release on the `development` branch.

| Version  | Supported          |
| -------- | ------------------ |
| ≥ 1.7.x  | ✅ Active          |
| < 1.7.x  | ⚠️ Best effort     |
| 0.x.x    | ❌ Not supported   |

> **Note:** AHA! Lang does not yet have a stable release channel. Use caution
> in production deployments and report any issues you encounter.

---

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

### Preferred: GitHub Security Advisory (private)

Open a private vulnerability report directly on GitHub:

👉 [Report a vulnerability](https://github.com/qwetls/aha-lang/security/advisories/new)

This creates a private channel between you and the maintainers. GitHub keeps
the report confidential until a fix is published.

### Alternative: Email

If you cannot use GitHub advisories, email us at:

**[security@xeycompany.com](mailto:security@xeycompany.com)**

Use the subject prefix `[SECURITY]` so the message is prioritized.

---

## What to Include in a Report

Please provide as much of the following as possible:

1. **Description** — A clear summary of the vulnerability.
2. **Affected version(s)** — The AHA! version or commit hash.
3. **Proof of concept** — Steps to reproduce, a minimal `.aha` source file, or
   a crash log.
4. **Impact** — What an attacker could achieve (memory corruption, arbitrary
   code execution, denial of service, etc.).
5. **Suggested fix** — If you have an idea, we'd love to hear it.

---

## Scope

### In scope

| Area | Examples |
|------|----------|
| **Compiler / codegen** | LLVM IR verification errors, miscompilation that can be triggered by untrusted `.aha` source |
| **Runtime** | Buffer overflows, use-after-free, double-free in `src/runtime.rs` builtins (`http_*`, `json_*`, `str_*`, file I/O) |
| **Network builtins** | Integer overflow in port numbers, unbounded `recv` without size limit |
| **AOT linker** | Command injection in `cc` invocation path |

### Out of scope

| Area | Why |
|------|-----|
| Vulnerabilities in dependencies (LLVM, Rust std) | Track upstream; report to the dependency's maintainer |
| Vulnerabilities introduced by user-supplied `.aha` programs | AHA! has no sandbox; a running program can already do what its process can do |
| Issues in `editors/` plugins (VS Code, etc.) | Report on the respective plugin repository |
| The documentation website (`aha-lang-docs`) | Not part of the compiler binary |

---

## Disclosure Policy

1. **Acknowledgement** — We will confirm receipt within **72 hours**.
2. **Triage** — We assess severity and determine a fix plan within **7 days**.
3. **Patch** — A fix is prepared on a private branch. The reporter is invited to
   review it.
4. **Coordinated release** — The fix and a security advisory are published
   together. The reporter's name/handle is credited (with permission).
5. **Embargo** — We ask reporters to refrain from public disclosure until the
   fix is released, or **90 days** after the initial report — whichever comes first.

---

## Security Best Practices for AHA! Users

* **Never run untrusted `.aha` programs** on systems that have sensitive
  credentials or network access — the AHA! runtime uses raw syscalls
  (`socket`, `bind`, `recv`, `send`) and has no sandbox.
* **Keep LLVM up to date** — vulnerabilities in the LLVM toolchain affect
  AOT-compiled binaries.
* **Review `http_listen` usage** — HTTP servers built with F11 builtins have no
  TLS, no authentication, and no rate limiting. Do not expose them to the
  public internet without a reverse proxy.

---

*Last updated: 2026-09-10*
