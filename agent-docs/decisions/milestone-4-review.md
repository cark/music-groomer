# Milestone 4 final review corrections

Status: alignment in progress; the items below are accepted but not yet
implemented as a correction set.

- Create result files and directories with ordinary process defaults governed
  by the invoking user's umask. Preserve contents, not source permissions,
  ownership, timestamps, ACLs, or extended attributes. Fresh mtimes make the
  groomed result easy for Navidrome to discover and honestly newly added.
- Keep source artwork paths as `PathBuf` through planning and Apply. Convert
  only for display with a non-panicking lossy renderer; test a non-UTF-8 Unix
  path through both preview and Apply.
- Use the existing progress boundary to inject failure at every Apply stage;
  prove cleanup and absence of a final destination without adding a general
  fault-injection framework.
- Add a complete temporary-fixture Apply proof for archive artwork replacing a
  source cover while the original is preserved byte-for-byte under
  `original-artwork/`.
- Fold the already authorized and completed Evolution exercise into Milestone
  4 instead of retaining a ceremonial Milestone 5. Correct all current-status
  documentation before acceptance.
- Remove the obsolete hidden demo implementation and its demo-only core state.
  Retain only genuinely unique assertions in the real planning or guided tests.
- Expand `~` from a non-empty `HOME` on every platform first, preserving custom
  Windows homes such as `E:/home`; fall back to `directories::BaseDirs` only
  when `HOME` is absent or empty. Test the precedence as pure logic.
