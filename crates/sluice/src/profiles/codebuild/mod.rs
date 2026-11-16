//! The check seam's launched profile on an AWS CodeBuild reserved-capacity
//! fleet (88, 107, C.3's launched row; record 0119; the change
//! `a-codebuild-fleet-runs-a-queues-checks`): a run is one build of its
//! system's run project, which holds no key, and its end starts a build of
//! the sign project, which runs none of the combination's code.

pub mod aws;
#[cfg(test)]
pub mod fake;
pub mod sdk;
