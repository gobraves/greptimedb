// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::HashMap;

use async_trait::async_trait;
use common_query::Output;
use common_telemetry::timer;
use servers::query_handler::ScriptHandler;

use crate::instance::Instance;
use crate::metrics;

#[async_trait]
impl ScriptHandler for Instance {
    async fn insert_script(
        &self,
        schema: &str,
        name: &str,
        script: &str,
    ) -> servers::error::Result<()> {
        let _timer = timer!(metrics::METRIC_HANDLE_SCRIPTS_ELAPSED);
        self.script_executor
            .insert_script(schema, name, script)
            .await
    }

    async fn execute_script(
        &self,
        schema: &str,
        name: &str,
        params: HashMap<String, String>,
    ) -> servers::error::Result<Output> {
        let _timer = timer!(metrics::METRIC_RUN_SCRIPT_ELAPSED);
        self.script_executor
            .execute_script(schema, name, params)
            .await
    }
}
