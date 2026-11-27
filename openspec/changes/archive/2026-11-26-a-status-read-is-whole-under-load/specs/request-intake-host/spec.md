## ADDED Requirements

### Requirement: A request that left the follows stays out during a read

A read of a followed pull request from GitHub SHALL put what it read only
into the follow that the read claimed. A request that a status or a wait
let go during that read SHALL stay out of the host's follows. A request
that another process followed again or read since SHALL keep what that
process wrote (137, design D3).

#### Scenario: Let go during a read

- **WHEN** a reading on `a` reads `#12` from GitHub and finds it
  merged, and a status on `a` says `#12` landed and lets it go before
  that read ends
- **THEN** `a`'s `forge/follows/` names `#12` no longer, and the next
  status shows no row for it
  (`a_follow_let_go_while_it_is_read_stays_let_go` in
  `crates/sluice/src/profiles/github/pulls.rs`)
