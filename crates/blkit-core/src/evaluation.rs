use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use serde_json::Value;

pub type Values = HashMap<String, Value>;
pub type Evaluate = Arc<dyn Fn(&Value, &Values) -> Result<Value, String> + Send + Sync>;
pub type AsyncEvaluate = Arc<
    dyn Fn(Value, Values) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>
        + Send
        + Sync,
>;
pub type Cancel = Arc<dyn Fn() + Send + Sync>;

pub fn next_retry_at(
    policy: &crate::RetryPolicy,
    attempt: u32,
    first_failure_ms: i64,
    now_ms: i64,
) -> Option<i64> {
    if attempt == 0 || attempt > policy.max_retries {
        return None;
    }
    let window_end =
        first_failure_ms.checked_add(i64::try_from(policy.retry_for.as_millis()).ok()?)?;
    let multiplier = 1_i64.checked_shl(attempt - 1)?;
    let delay = i64::try_from(policy.retry_delay.as_millis())
        .ok()?
        .checked_mul(multiplier)?;
    let eligible = now_ms.checked_add(delay)?;
    (eligible <= window_end).then_some(eligible)
}
