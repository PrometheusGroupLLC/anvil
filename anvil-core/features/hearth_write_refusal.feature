Feature: A write that could not read its own input refuses, and destroys nothing

  # ── C-d.1 round 9, H-1 of `review-cd1-fixround-r8.md` ────────────────────
  #
  # Round 8 fixed eight read-then-write sites. The round-8 reviewer reverted the
  # highest-blast-radius one — `register_mcp`'s
  # `read_to_string(&path).unwrap_or_default()` — verbatim, and ran everything:
  #
  #   anvil-core     232 features / 1765 scenarios / 1765 PASSED / 0 failed
  #   anvil-engine   142 features /  533 scenarios /  529 passed / 4 failed (baseline)
  #
  # ZERO RED. A user's `kiln` and `lore` MCP registrations and every unrelated
  # key in their own `.claude.json`, destroyed on their own machine with
  # `written` reported, could be reintroduced by anyone and nothing would notice.
  # The read half of this round got 122 new cells; the write half got none. On a
  # track whose entire subject is assertions that cannot fail, eight fixes with
  # zero cells is that same defect one step further out.
  #
  # WHAT EVERY ROW HOLDS CONSTANT: one fixture, one named node, and only that
  # node's MODE varies. The step module refuses to evaluate any row whose fixture
  # did not seed the thing the row claims survived — so a "still holds kiln" row
  # cannot pass over a file that never held kiln, and a "no phantom" row cannot
  # pass over a hearth that already had one.
  #
  # WHY THE CONTROLS ARE THE POINT. `written` at 0644 must also prove anvil's own
  # entry ARRIVED; otherwise a `register_mcp` that wrote nothing at all would
  # satisfy every refusing row. `filed` at 0300 must prove the event reached the
  # artifact's REAL history; 0300 is traversable and not listable, so a blanket
  # "any mode that is not 0755 refuses" fails there.
  #
  # WHY THE REFUSAL ROWS ASSERT A TOKEN AND NOT MERELY AN ERROR. At mode 0000 the
  # pre-fix code ALSO failed — but it failed at the write, having already decided
  # to write a document it had read as empty, or having already invented an
  # artifact location. "Refused because it could not read its input" and
  # "happened to fail later anyway" are different facts, and only the first is
  # the fix.
  #
  # SCOPE, DECLARED. Round 8 converted eight sites; these are the two whose blast
  # radius is OTHER PEOPLE'S DATA. The other six are declared EIO-class-only in
  # `implementation-c.md` §41.9(4) — their swallowing arm is reachable only by an
  # error class no local POSIX fixture can produce, because the mode that defeats
  # the read also defeats the temp-sibling write beneath it.

  # ── A: the installer's MCP registration, over the user's own config ──────
  #
  # `install_one(.., with_mcp: true)` reads the harness's MCP config, asks the
  # pure writer to add anvil's entry, and writes the whole document back. The
  # read was `read_to_string(&path).unwrap_or_default()`; `parse_json("")` is an
  # empty map; so an unreadable config produced a document holding ANVIL ALONE
  # and `write_config` put it over the real one. `std::fs::write` needs write
  # permission and not read permission, so mode 0200 reaches it with nothing
  # exotic. The hook config (`settings.json`) is a different file and stays
  # readable on every row, so nothing here refuses for the wrong reason.
  Scenario Outline: install_one over an MCP config at mode "<mode>" reports "<outcome>" and preserves "<survivors>"
    Given a Claude Code config dir whose MCP config holds two foreign servers and an unrelated setting, at mode "<mode>"
    When the hook installer runs for Claude Code with MCP registration
    Then the install reports "<outcome>" and the MCP config still holds "<survivors>"

    Examples:
      | mode | outcome | survivors        |
      | 0644 | written | kiln,lore,theme  |
      | 0200 | failed  | kiln,lore,theme  |
      | 0000 | failed  | kiln,lore,theme  |

  # The CONTROL's own control: at 0644 the installer must really have written,
  # or "preserved kiln" above is satisfied by a call that did nothing at all.
  Scenario: the readable control really registers anvil
    Given a Claude Code config dir whose MCP config holds two foreign servers and an unrelated setting, at mode "0644"
    When the hook installer runs for Claude Code with MCP registration
    Then the install reports "written" and the MCP config still holds "kiln,lore,theme,anvil"

  Scenario Outline: install_one refuses by name over an MCP config at mode "<mode>"
    Given a Claude Code config dir whose MCP config holds two foreign servers and an unrelated setting, at mode "<mode>"
    When the hook installer runs for Claude Code with MCP registration
    Then the install refusal names "mcp_config_uninspectable"

    Examples:
      | mode |
      | 0200 |
      | 0000 |

  # ── B: the transition-event write, over an uninspectable per-kind dir ────
  #
  # `append_transition_event` by BARE ID resolves the artifact through a per-kind
  # fallback loop that carried `candidate.exists()` — the write-half twin of the
  # lookup round 6 fixed on the read side, untouched by rounds 6 and 7 because
  # every list was a list of READERS. `exists()` answering false for an
  # uninspectable `tracks/` returned "not here", the caller's
  # `unwrap_or_else(|| hearth.join(artifact_path))` invented a location at the
  # hearth ROOT, and the `create_dir_all` on the next line made it real — while
  # the comment above it claimed the fallback existed so an absent directory
  # would surface as an error.
  Scenario Outline: filing a transition with the per-kind directory at mode "<mode>" reports "<outcome>"
    Given a hearth holding a track artifact with the per-kind directory at mode "<mode>"
    When a governance transition is filed for the artifact by bare id
    Then the transition write reports "<outcome>" with "<events>" event(s) in the artifact's own history and no phantom artifact

    Examples:
      | mode | outcome | events |
      | 0755 | filed   | 1      |
      | 0500 | filed   | 1      |
      | 0300 | filed   | 1      |
      | 0600 | refused | 0      |
      | 0000 | refused | 0      |

  Scenario Outline: the transition write refuses by name with the per-kind directory at mode "<mode>"
    Given a hearth holding a track artifact with the per-kind directory at mode "<mode>"
    When a governance transition is filed for the artifact by bare id
    Then the transition refusal names "artifact_location_uninspectable"

    Examples:
      | mode |
      | 0600 |
      | 0000 |
