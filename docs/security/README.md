# Security documentation

This directory tracks security findings against the toy AMM, our written responses, and classroom exercises derived from the same material.

## Layout

```
docs/security/
├── issues/      # vulnerability reports (one per finding)
├── responses/   # the corresponding mitigation plans
├── exercises/   # classroom exercises framed as questions
└── README.md    # this file
```

Files are numbered sequentially (`001-`, `002-`, ...). The number is the same across `issues/`, `responses/`, and any related exercises: issue 001 has response 001 and exercise 001 if they exist. The slug after the number names the bug, not the mitigation.

## Conventions

- **Issues** open with a metadata table (status, severity, component, PoC pointer, spec section) and contain a summary, vulnerability description, PoC reference, impact, and high-level recommendation. They do *not* prescribe an implementation; that's the response's job.
- **Responses** open with a similar metadata table that references the issue, then commit to a specific mitigation, list alternatives considered, give a concrete implementation plan, and call out open questions. The response is the canonical record of *what we decided*; the issue is the canonical record of *what was found*.
- **Exercises** are derived from issues but framed pedagogically: present an artifact (usually a captured `print_logs_structured()` trace), pose a question, give sub-questions for stuck students, and hide the walkthrough behind a collapsible `<details>` block.

## Current entries

| ID  | Status                    | Severity | Title                                                                            |
|-----|---------------------------|----------|----------------------------------------------------------------------------------|
| 001 | Open (mitigation planned) | High     | [Lock/unlock timing attack](issues/001-lock-unlock-timing-attack.md)             |

Each row above links to the issue; the matching response lives at `responses/<same-slug>.md`, and the classroom exercise (when one exists) lives at `exercises/<same-number>-<question-shaped-slug>.md`.

## Reproducing a PoC

PoC tests live under `programs/amm/tests/`. They are written so they currently pass against the unpatched implementation (i.e., they demonstrate the bug; a passing test is the evidence). Reproduce with:

```sh
just poc                                          # the named PoC: lock/unlock attack
just tt --test <test_file_name>                   # any other PoC, with structured logs
cargo test -p amm --features amm/test-helpers --test <test_file_name> -- --nocapture
```

After a mitigation lands, the matching PoC test is updated to assert the *mitigated* behavior (the attack tx now fails). The original failing-from-honest-user assertions stay; what changes is the line that previously asserted the attack succeeded.
