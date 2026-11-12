//! The add-account flow.

use super::AddFlowStatus;
use crate::types::SetupStatus;

/// The add-flow status of a setup. The backend always sends every field; an
/// empty one becomes `None` here, which is how every caller reads it.
pub fn to_add_flow_status(setup_id: &str, payload: &SetupStatus) -> AddFlowStatus {
    let optional = |value: &str| (!value.is_empty()).then(|| value.to_string());
    AddFlowStatus {
        setup_id: setup_id.to_string(),
        state: payload.state.clone(),
        account_id: optional(&payload.account_id),
        account_display_name: optional(&payload.account_display_name),
        error_message: optional(&payload.error_message),
    }
}
