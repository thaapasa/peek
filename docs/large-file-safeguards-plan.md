# Large File Safeguards — access-unlock design (items 1 + 3)

> **Status: In-progress plan (2026-06-13).** Design for the remaining
> Large File Safeguards work — the latency confirmation prompt and the
> info-default / opt-in-load UX, unified into one session-unlock model.
> The memory guards already shipped; see
> [planned.md → Large File Safeguards](planned.md#large-file-safeguards-)
> and the strategy doc [memory-streaming.md](memory-streaming.md). Move to
> `archived/` (or delete) when landed.

## Goal

Two remaining pieces from the [two-threat model](memory-streaming.md#two-threats):

- **Latency** — a confirmation before a slow-but-bounded op (big bare-codec
  decompress, big compressed-tar TOC build, deep compressed seek).
- **Info-default / opt-in** — when opening a file whose primary view needs
  a guarded op, land on the Info screen with a "press [key] to load"
  prompt instead of doing the work up front.

Unify both under one idea: a **session-wide access unlock**.

## The model: one session unlock

`ViewerState` carries an access tier:

```
enum Access { Default, Unlocked }
```

- The **first** guarded op in a session prompts. On confirm → `Unlocked`
  for the rest of the session; no re-ask. Permission is granted once and
  covers every later slow/large op.
- **Non-interactive** paths (pipe / print / `--info` / `--list`) start
  `Unlocked` — a pipe can't answer a prompt, and the caller chose the op.
- A CLI flag (`--yes` / `--force`, name TBD) starts an interactive session
  `Unlocked` too — pre-granted, no prompts.

The unlock is **session-wide**: it persists across descend frames (push a
huge archive entry, then a second — only the first asks).

## What unlock grants (mind the risk asymmetry)

| Effect                                                                             | Risk                                                 | Recommendation                                                                                                                                                      |
|------------------------------------------------------------------------------------|------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Latency** — slow ops proceed (decompress, full TOC scan, deep seek)              | none (just time)                                     | always grant on unlock                                                                                                                                              |
| **Disk** — raise/remove the spill-path disk ceiling (the decompression-bomb guard) | low (ENOSPC, not crash)                              | grant on unlock                                                                                                                                                     |
| **Memory** — non-spillable hard caps move Default → Elevated tier                  | real (OOM/crash for transform / random-access parse) | **the one explicit knob**; keep an absolute max even when unlocked; prefer leaving whole-doc-transform caps on their soft-degrade placeholder rather than elevating |

Key invariant: **even Unlocked, an absolute ceiling holds** so peek can
never be driven to a hard crash. "Two classes of hard limit" = `Default`
(conservative) and `Elevated` (risk-accepted), both bounded; nothing
becomes infinite.

## Two-tier caps

`limits.rs` grows, for each *elevatable* class, a `Default` and an
`Elevated` value plus an absolute ceiling. The active tier is selected by
`Access`. Spillable paths (decompress, extract) mostly need only a **disk**
ceiling lifted, not a memory tier — they already bound RAM by their spool
threshold.

## Where the decision lives (coherence — see caveat 2)

The leaf guards stay **pure**: they receive a cap, they never read session
state. The tier is threaded down **as data**, reusing existing carriers:

- `ComposeOpts` (already flows bin → `compose_modes` → per-type compose)
  carries the `Access`/tier.
- `ExtractOptions` (already carries `no_tempfile`) carries it for the
  extract / descend path.
- `run_view` passes it into the transparent-decompress deferral.

**This is the same plumbing as Option A** (budget-required reads): a
`Budget` resolves to a concrete cap *given the tier*. Do Option A and this
together — a bare `read_bytes()` becomes `read_bytes(Budget)`, where
`Budget` + session tier → cap. One refactor, not two.

## The eager-decompress deferral (coherence — see caveat 3)

`resolve_transparent` runs in `run_view` **before** the session exists, so
a top-level `huge.xz` is already decompressed before any prompt could show.
Fix: defer in the bin.

```
run_view:
  if detected is Compressed(fmt)
     && compressed byte_len > LATENCY_PROMPT_BYTES
     && interactive && access == Default:
        // skip eager resolve_transparent
        build a frame over the COMPRESSED wrapper, marked Deferred::Decompress(fmt)
        → session lands on Info (codec, compressed size) + "press [key] to load"
  else:
        resolve_transparent eagerly (small/common path, or pre-authorized)
```

On the load key: run `decompress_to_source`, re-detect the inner content,
rebuild the frame (same path the retry-detect / descend reseed uses).
`peek-detect` stays pure — only the bin knows `interactive` / the flag.

The same gate applies in `push_extracted` (descend), but there `access`
may already be `Unlocked`, so it just proceeds.

## Prompt points (interactive)

- **Open** — the primary view needs a guarded op (deferred decompress; a
  whole-load mode over its Elevated cap): land on **Info**, with a status /
  prompt line `press [key] to load (N MB, slow)`. Key → unlock → run the op
  → switch to the real primary view. Reuses the deferred-primary idea (a
  `Mode::loads_whole_file()`-style signal + a frame `deferred` slot).
- **Descend (Enter)** — the entry's extract / decompress is guarded: a
  **modal prompt before extraction** (reuse the existing `ViewerState`
  prompt slot + `PromptKind`). Confirm → unlock → proceed with the descend.

## Latency predicate

"Expensive" ≈ a compressed / sequential source whose **compressed**
`byte_len` exceeds `LATENCY_PROMPT_BYTES` (start ~50 MB, tune). Evaluated
where the op is about to run: the `run_view` deferral, the archive
compressed-TOC build, the extract path. Random-access formats (zip / plain
tar / ISO) never trip it.

## Guarded-op map

| Op                                         | Spillable?         | Default guard today            | Unlock effect                                                |
|--------------------------------------------|--------------------|--------------------------------|--------------------------------------------------------------|
| Top-level bare-codec decompress            | yes (RAM bounded)  | eager, no prompt               | defer + prompt when compressed > threshold; unlock = proceed |
| Compressed-tar TOC build                   | no (full pass)     | runs silently                  | prompt when compressed > threshold                           |
| Archive entry extract / descend            | yes (spool ≥16 MB) | runs; disk uncapped            | prompt on large; unlock lifts disk ceiling                   |
| Whole-doc transform render                 | no                 | soft placeholder ≥32 MB        | (optional) Elevated cap; else keep placeholder               |
| Random-access parse (objfile/EPS/notebook) | no                 | hard refuse / degrade over cap | (optional) Elevated cap, absolute max holds                  |

## Open decisions

- **Elevate memory caps, or latency + disk only?** Recommend latency +
  disk on unlock; treat memory-tier elevation as a separate opt-in (or skip
  it, leaving transform caps on soft-degrade). Avoids turning a "wait
  longer" grant into an OOM.
- Threshold numbers: `LATENCY_PROMPT_BYTES`, the spill disk ceiling, the
  Elevated tier values + absolute max.
- CLI flag name (`--yes` / `--force` / `--large`).
- Does the deferred-open reuse the descend reseed path, or a new one?
- Interaction with the existing retry-detect rebuild (`reseed_from_modes`).

## Sequencing

1. **Option A first** (budget-required reads + the ~44-site audit) — lays
   the tier-aware `Budget` plumbing this design rides on.
2. Session `Access` state + non-interactive default + CLI flag.
3. `run_view` deferral + the Info-default open prompt.
4. Descend / extract prompt.
5. Optional memory-tier elevation + the spill disk ceiling.
