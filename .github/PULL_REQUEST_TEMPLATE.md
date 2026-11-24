## Summary
- Describe what changed and why.
- Related issue(s): `#...`

## Verification Checklist
- [ ] `cargo test --workspace` passes with zero failures
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes cleanly
- [ ] `cargo check --workspace --locked` succeeds
- [ ] If `driver/` changed: verified DKMS/kernel module build (`hp-wmi.ko` & `hp-omen-extra.ko`)
- [ ] If UI strings changed: updated TR, EN, and ES keys in `src/omen-gui/src/i18n.rs`
- [ ] Respects split-privilege architecture (no root operations in GUI/CLI/tray/overlay; hardware writes stay in `omen-space-daemon`)
