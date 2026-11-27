## ADDED Requirements

### Requirement: A pass's report comes before the document read after it

`sluice host` SHALL send a folder's report of a pass before any document
that it read after that pass. So a client that reads a document has the
report of each pass before it. A wait that ends on a document says what
each of those passes released (122, 123).

#### Scenario: A release fails after a landing

- **WHEN** a round lands a submission, the release of its checkout then
  fails, and a waiting submit follows the submission
- **THEN** the submit prints the failed release and the landing, and
  exits 0
  (`a_release_failing_after_the_landing_leaves_the_submit_landed` in
  `crates/sluice/tests/landed.rs`)

#### Scenario: A report and a document ready at one time

- **WHEN** a pass's report and the document read after it are both
  ready to send
- **THEN** the client gets the report first
  (`a_report_goes_before_the_document_read_after_it` in
  `crates/sluice/src/host_process/subscription.rs`)
