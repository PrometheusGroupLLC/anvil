use crate::domain::describe::{DescribeError, TransitionInfo};

/// Data returned by the port for instance-level describe.
#[derive(Debug, Clone)]
pub struct InstanceState {
    pub kind: String,
    pub state: String,
    pub transition_count: usize,
    pub last_transition: Option<TransitionInfo>,
}

/// Port for describe queries.
pub trait DescribePort {
    fn read_instance(&self, artifact_id: &str) -> Result<InstanceState, DescribeError>;
}
