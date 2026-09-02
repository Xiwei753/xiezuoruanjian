use super::*;

impl WriterCoreApi {
    pub fn list_registered_actions(
        &self,
    ) -> ApiResult<Vec<crate::api::types::ActionDescriptorDto>> {
        self.core_write()
            .list_registered_actions()
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn execute_action_ext(
        &self,
        action_id: &str,
        args_json: &str,
        context_json: &str,
    ) -> ApiResult<crate::api::types::ActionResultDto> {
        self.core_write()
            .execute_action(action_id, args_json, context_json)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn ai_available(&self) -> bool {
        self.core_write().ai_available()
    }

    pub fn list_registered_actions_json(&self) -> String {
        let result: ApiResult<Vec<crate::api::types::ActionDescriptorDto>> =
            self.list_registered_actions();
        ResultEnvelope::from_api_result(result).to_json_string()
    }

    pub fn execute_action_json(
        &self,
        action_id: &str,
        args_json: &str,
        context_json: &str,
    ) -> String {
        let result: ApiResult<crate::api::types::ActionResultDto> =
            self.execute_action_ext(action_id, args_json, context_json);
        ResultEnvelope::from_api_result(result).to_json_string()
    }
}
