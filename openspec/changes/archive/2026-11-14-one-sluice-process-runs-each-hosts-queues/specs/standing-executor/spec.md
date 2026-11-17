## RENAMED Requirements

- FROM: `### Requirement: sluice run claims runs its signer counts for`
- TO: `### Requirement: The host process claims runs its signer counts for`

## MODIFIED Requirements

### Requirement: The host process claims runs its signer counts for
sluice's host process on a host with `[signing]` SHALL watch the state of
every branch with a remote in the repositories it holds, and claim open
runs of its systems on branches whose gate trusts its signer, up to the
host's bound, while it runs (135). It SHALL claim no run that its
signer's results could not satisfy.

#### Scenario: A session host and main
- **WHEN** a host signs as `sessions` and `main` trusts only `box` and `forge`
- **THEN** the host process on that host claims no run of `main`, and claims runs of a bolt whose `trust` lists `sessions`

#### Scenario: A confined gate and an unconfined signer
- **WHEN** `main` has `confined = true`, trusts `box`, and `box` has `confined = false`
- **THEN** the box's host process claims no run of `main`
