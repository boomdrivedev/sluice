## ADDED Requirements

### Requirement: A forge signer signs keylessly and sluice verifies it
A `github` signer SHALL sign a run's statement keylessly with the
signing job's workload identity, through the sigstore instance GitHub
uses for the repository. sluice SHALL verify such a record on every host
that reads it, from the bundle alone and the instance's trusted root
cached on the host, and SHALL count it only when the certificate's
workflow, ref, repository and issuer match `[signers.<name>] github`
(78, 79).

#### Scenario: A signing job on main
- **WHEN** `sluice.yml`'s sign job on `main` signs a pass of `cli:build` on `linux-x64`, and `[signers.forge] github` names that workflow on `main`
- **THEN** every host reads the pass as signed by `forge`

#### Scenario: A pull request's workflow
- **WHEN** a bundle's certificate names `sluice.yml` on a pull request's ref
- **THEN** the record counts for nothing, and `sluice status` says which identity it carried

#### Scenario: A host with no trusted root
- **WHEN** a host cannot fetch the instance's trusted root and has none cached
- **THEN** it reads the record as unverified, never as a pass, and says why
