# AGENTS.md

This repo is part of the owner's pebbles-hq workspace. **Read the full handoff first:**
`/home/reyco/xreyc/pebbles-hq/AGENTS.md` (workspace root; the same text is committed in the Gravel repo as `AGENTS.md`).

Essentials:
- Build/test only via `~/.cache/phasec/safe-cargo …` (low-RAM laptop; one heavy job at a time). GUI tests on Xvfb + lavapipe: `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan`.
- Run only the tests affected by your change; never the full suite unless asked. Don't prune `target/` unless ≤ 50 GB disk free.
- Commit as `Reyco Seguma <segumareyco@gmail.com>`; push as xreyc: `git -c credential.helper= -c credential.https://github.com.helper= push "https://xreyc:$(gh auth token -u xreyc)@github.com/pebbles-hq/<repo>.git" <ref>`, then verify with an authenticated `git ls-remote`.
- Pebbles: work in the `wt-lcd` worktree (local branch `lcd`), push `HEAD:titlebar`; never push `main` directly. pebbles-code-editor: branch `linux6-stress`.
- After changing Pebbles or the editor, repin Gravel (`/home/reyco/xreyc/seguma-hq/gravel`) to the pushed revs; Gravel's Cargo.lock must contain exactly one pebbles rev.
- Match Flutter/Skia/CSS semantics; performance bar: beat VS Code and Zed (no IO/parsing in render paths). Never edit pebbles-docs.
- Checklists and status: `/home/reyco/xreyc/pebbles-hq/documentations/`.
