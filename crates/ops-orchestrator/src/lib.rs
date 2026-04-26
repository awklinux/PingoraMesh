use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    VipBind,
    VipUnbind,
    RouteSwitch,
    ServiceReload,
    CustomTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationTemplate {
    pub name: String,
    pub kind: OperationKind,
    pub command_template: String,
    pub allowed_params: Vec<String>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Error)]
pub enum OperationValidationError {
    #[error("unknown parameter: {0}")]
    UnknownParam(String),
    #[error("invalid parameter value for: {0}")]
    InvalidValue(String),
    #[error("missing placeholder value for: {0}")]
    MissingValue(String),
}

impl OperationTemplate {
    pub fn render_command(
        &self,
        params: &Map<String, Value>,
    ) -> Result<String, OperationValidationError> {
        let allowed: HashSet<&str> = self.allowed_params.iter().map(String::as_str).collect();

        for key in params.keys() {
            if !allowed.contains(key.as_str()) {
                return Err(OperationValidationError::UnknownParam(key.clone()));
            }
        }

        let mut rendered = self.command_template.clone();
        for key in &self.allowed_params {
            let placeholder = format!("{{{{{key}}}}}");
            if rendered.contains(&placeholder) {
                let value = params
                    .get(key)
                    .ok_or_else(|| OperationValidationError::MissingValue(key.clone()))?;
                let replacement = match value {
                    Value::String(v) => v.clone(),
                    Value::Number(v) => v.to_string(),
                    Value::Bool(v) => v.to_string(),
                    _ => return Err(OperationValidationError::InvalidValue(key.clone())),
                };
                rendered = rendered.replace(&placeholder, &replacement);
            }
        }

        Ok(rendered)
    }
}
