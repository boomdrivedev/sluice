//! Signed run records (78, 79, 109; C.6; ADR 0032): a run's results as
//! an in-toto statement in a DSSE envelope, which every host verifies
//! from the records and the policy alone.
//!
//! This module knows the envelope and the statement, and how an SSH key
//! or a key in a key service signs one. Which identities a branch's policy names, and a forge's
//! keyless signatures, are the records seam's and the forge profile's.

pub mod dsse;
pub mod kms;
pub mod ssh;
pub mod statement;
