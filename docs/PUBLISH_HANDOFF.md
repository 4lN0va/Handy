# Handoff: publishing this fork

Written for whoever runs the push. Everything below is a command to execute, not a
suggestion to evaluate. Where a step can fail, it says how.

All work is already committed locally on `feature/voice-api-mvp`.

---

## STOP — verify you are in the right repository first

These are **two different repositories on the same machine**, and they are easy to
confuse because both are forks under the same account:

| Repository | Path (Windows) | Path (WSL) | Remote |
|---|---|---|---|
| **This fork — the one to publish** | `C:\Users\Nova\Project Brief\handy-fork` | `/mnt/c/Users/Nova/Project Brief/handy-fork` | `4lN0va/Handy` |
| The verification tool (a different project) | `E:\Default\Desktop\handy-integration-verification-tool` | `/mnt/e/Default/Desktop/handy-integration-verification-tool` | `4lN0va/handy-integration-verification-tool` |

**`/mnt/e/...` is not this repository.** Working there pushes the verification tool
to the verification tool's own remote, and `4lN0va/Handy` stays untouched.

The path contains a space, so it must be quoted:

```bash
cd "/mnt/c/Users/Nova/Project Brief/handy-fork"
```

### Gate: run this before pushing anything

```bash
git rev-parse --show-toplevel          # .../handy-fork
git rev-parse --abbrev-ref HEAD        # feature/voice-api-mvp
git remote get-url origin              # https://github.com/4lN0va/Handy.git
git rev-list --count upstream/main..HEAD   # 16
git status --porcelain                 # no output
```

All five must match. If `origin` shows `handy-integration-verification-tool`, you
are in the wrong repository — stop.

### "0 commits ahead" on `4lN0va/Handy` is expected, not a problem

Nothing has ever been pushed. `4lN0va/Handy` being byte-identical to upstream is
the **starting state**: the 16 commits exist only in this local working copy. That
is precisely what this handoff exists to change.

If a check of the GitHub side shows `0 commits ahead`, no `PUBLISH_HANDOFF.md` and
no commits, that is consistent with an unpushed local branch. It is not evidence
that the work is missing or that the wrong repository has been found.

---

## 0. Preconditions

| Check | Command | Expected |
|---|---|---|
| Remotes | `git remote -v` | `origin` → `https://github.com/4lN0va/Handy.git`, `upstream` → `https://github.com/cjpais/Handy.git` |
| Working tree clean | `git status --porcelain` | no output |
| Commits to publish | `git rev-list --count upstream/main..HEAD` | **15** |
| Push access | `gh auth status` | must be logged in as **`4lN0va`** |

**The access check is the one that fails.** `gh` on the machine where this was
prepared is logged in as `axiscorporation2015-stack`, which has `pull: true` and
`push: false` on `4lN0va/Handy`:

```bash
gh api repos/4lN0va/Handy --jq .permissions     # {"admin":false,"maintain":false,"pull":true,"push":false,...}
```

Pushing as that account returns `403`. Log in as the owner first:

```bash
gh auth login          # choose 4lN0va
gh auth status         # confirm the active account
```

---

## 1. Push the feature branch first

```bash
cd <repo>
git push -u origin feature/voice-api-mvp
```

Pushing the branch before touching `main` keeps the first CI run isolated: if a
workflow fails, nothing on the default branch is affected.

**Watch the Actions run that this triggers.** `feature/voice-api-mvp` is not
`main`, so `main-build.yml` (which only runs on `main`) will not fire. Expected:

| Workflow | Expected on this push |
|---|---|
| `voice-api.yml` | should pass — crate tests, fmt, clippy on 3 platforms |
| `test.yml` | should pass — upstream unit tests |
| `code-quality.yml` | should pass |
| `playwright.yml` | **may fail** — upstream front-end E2E, and this fork removed the Voice API settings page and its sidebar entry |

If `playwright.yml` fails, that is expected and not a regression: the tests assert
against UI this fork intentionally changed. Either update them or delete the
workflow; do not treat it as blocking.

---

## 2. Merge into `main` and push

Only after §1's `voice-api.yml` is green.

```bash
git checkout main
git merge --no-ff feature/voice-api-mvp -m "Merge fork features into main"
git push -u origin main
```

This is the push that starts the real build.

### What will run on `main`

| Workflow | Platform coverage |
|---|---|
| `main-build.yml` | **macOS (aarch64 + x86_64), Windows (x86_64), Linux (x86_64)** — the three-platform build |
| `voice-api.yml` | macOS, Windows, Linux |
| `test.yml`, `code-quality.yml` | Linux |

`main-build.yml` uses **no secrets**. It produces unsigned artifacts, retained for
30 days. That is the intended outcome.

### If `main-build.yml` fails

It has never been run on this fork. The most likely first failure is the native
layer — `transcribe-cpp` (ggml with Vulkan/Metal/CPU) and ONNX Runtime — because
that is the part most sensitive to runner images.

Read the failing step before changing anything. The fork's own code adds no
platform-conditional statements (see `FORK_MAINTENANCE.md` §6), so a native build
failure is almost certainly an upstream-versus-runner issue rather than something
this fork introduced.

---

## 3. Workflows: what is there and why

`nix-check.yml` and `app-build.yml` were **deleted** before handoff.

| Kept | Note |
|---|---|
| `build.yml` | **Reusable** (`on: workflow_call`). Called by `main-build.yml`, `build-test.yml`, `pr-test-build.yml`, `release.yml`. Do **not** delete this one — removing it breaks the three-platform build, which is the reason this is a fork rather than a fresh repository. |
| `main-build.yml` | The three-platform build. No secrets. |
| `voice-api.yml` | Added by this fork. |
| `release.yml` | `workflow_dispatch` only; will not publish by itself. Producing installers needs signing secrets that do not exist here. |

---

## 4. Selecting this fork on GitHub (optional)

The repository is currently `4lN0va/Handy`, the same name as upstream. Renaming is
safe: GitHub redirects the old URL, and git remotes are unaffected.

```bash
gh repo rename Handy-Continuous        # or any name you prefer
git remote set-url origin https://github.com/4lN0va/Handy-Continuous.git
```

**Do not change the Tauri `identifier`** (`com.pais.handy` in
`src-tauri/tauri.conf.json`). It determines the application data directory, so
changing it orphans every downloaded model, the settings, the history and the
stored credentials. It is currently **not** changed.

`productName` is safe to change if the installed app should not be called "Handy",
but it is a separate decision and is also currently unchanged.

---

## 5. What is verified and what is not

Stated so nothing here is over-claimed.

**Verified on Windows x86_64 only:**

- 323 application-crate tests pass, plus 182 crate unit tests and 11 real-socket
  HTTP integration tests.
- The application builds and runs; real speech was transcribed end to end,
  including automatic pause segmentation.
- `apply_license_overrides`, `gen_model_licenses` and `gen_third_party_notices`
  CI guards pass.

**Not verified anywhere:**

- Any CI workflow, on any platform. None has been executed.
- Any macOS or Linux build or run.
- The signing and installer path (`release.yml`).
