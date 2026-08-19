use anvil_core::domain::describe::DescribeError;
use anvil_core::ports::describe_port::{DescribePort, InstanceState};
use std::collections::HashMap;

/// In-memory test adapter for describe queries.
#[derive(Debug, Clone)]
pub struct TestDescribeAdapter {
    instances: HashMap<String, InstanceState>,
}

impl TestDescribeAdapter {
    pub fn new(instances: HashMap<String, InstanceState>) -> Self {
        Self { instances }
    }
}

impl DescribePort for TestDescribeAdapter {
    fn read_instance(&self, artifact_id: &str) -> Result<InstanceState, DescribeError> {
        self.instances
            .get(artifact_id)
            .cloned()
            .ok_or_else(|| DescribeError::UnknownIdentifier {
                identifier: artifact_id.to_string(),
            })
    }
}
