//! sluice's host process (135, 129, 71, 150; record 0093; rules core.25,
//! core.31, core.49): the one process for a user on a host that moves
//! every repository's queues, holds the host's bound and reads each
//! repository for every watch, wait and status on the host.

pub mod room;
